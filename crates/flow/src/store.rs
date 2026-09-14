//! Persistencia de fluxos e do historico de execucoes em disco.
//!
//! Um arquivo JSON por fluxo e um por execucao. E simples de inspecionar, de
//! versionar e de copiar entre maquinas — que e o que se espera de um app local.
//! A escrita e atomica (arquivo temporario + rename) para que um desligamento
//! no meio da gravacao nao deixe um fluxo truncado.

use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use tokio::fs;

use crate::model::{Flow, FlowSummary, FLOW_SCHEMA};
use crate::run::{RunRecord, RunSummary};

/// Quantas execucoes ficam guardadas antes da limpeza automatica.
pub const DEFAULT_RUN_HISTORY: usize = 200;

/// Repositorio de fluxos e execucoes.
#[derive(Debug, Clone)]
pub struct FlowStore {
    flows_dir: PathBuf,
    runs_dir: PathBuf,
}

impl FlowStore {
    /// Cria o repositorio sob `root`, sem tocar no disco ainda.
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            flows_dir: root.join("flows"),
            runs_dir: root.join("flow-runs"),
        }
    }

    /// Garante que os diretorios existem.
    pub async fn ensure_dirs(&self) -> io::Result<()> {
        fs::create_dir_all(&self.flows_dir).await?;
        fs::create_dir_all(&self.runs_dir).await
    }

    pub fn flows_dir(&self) -> &Path {
        &self.flows_dir
    }

    /// Todos os fluxos, do mais recente para o mais antigo.
    pub async fn list(&self) -> io::Result<Vec<FlowSummary>> {
        let mut summaries: Vec<FlowSummary> = self
            .read_all()
            .await?
            .iter()
            .map(Flow::summary)
            .collect();
        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(summaries)
    }

    /// Todos os fluxos completos. Usado pelo roteador de webhooks e pelo
    /// agendador, que precisam olhar os parametros dos gatilhos.
    pub async fn read_all(&self) -> io::Result<Vec<Flow>> {
        if !self.flows_dir.exists() {
            return Ok(Vec::new());
        }

        let mut flows = Vec::new();
        let mut entries = fs::read_dir(&self.flows_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            match read_flow_file(&path).await {
                Ok(flow) => flows.push(flow),
                // Um arquivo corrompido nao deve derrubar a listagem inteira.
                Err(error) => tracing::warn!(path = %path.display(), %error, "fluxo ignorado"),
            }
        }
        Ok(flows)
    }

    /// Um fluxo pelo id.
    pub async fn get(&self, id: &str) -> io::Result<Option<Flow>> {
        let Some(path) = self.flow_path(id) else {
            return Ok(None);
        };
        if !path.exists() {
            return Ok(None);
        }
        read_flow_file(&path).await.map(Some)
    }

    /// Cria ou atualiza um fluxo, preenchendo id e carimbos de tempo.
    pub async fn save(&self, mut flow: Flow) -> io::Result<Flow> {
        self.ensure_dirs().await?;

        flow.schema = FLOW_SCHEMA.to_string();
        let existing = if flow.id.trim().is_empty() {
            flow.id = uuid::Uuid::new_v4().to_string();
            None
        } else {
            self.get(&flow.id).await?
        };

        let now = Utc::now();
        flow.created_at = existing
            .as_ref()
            .and_then(|previous| previous.created_at)
            .or(flow.created_at)
            .or(Some(now));
        flow.updated_at = Some(now);

        let path = self
            .flow_path(&flow.id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "id de fluxo invalido"))?;
        write_json_atomic(&path, &flow).await?;
        Ok(flow)
    }

    /// Remove um fluxo. `false` quando ele nao existia.
    pub async fn delete(&self, id: &str) -> io::Result<bool> {
        let Some(path) = self.flow_path(id) else {
            return Ok(false);
        };
        if !path.exists() {
            return Ok(false);
        }
        fs::remove_file(&path).await?;
        Ok(true)
    }

    /// Guarda uma execucao e apara o historico.
    pub async fn save_run(&self, run: &RunRecord) -> io::Result<()> {
        self.ensure_dirs().await?;
        let Some(path) = self.run_path(&run.id) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "id de execucao invalido",
            ));
        };
        write_json_atomic(&path, run).await?;
        self.prune_runs(DEFAULT_RUN_HISTORY).await
    }

    /// Historico, opcionalmente filtrado por fluxo, do mais recente ao mais antigo.
    pub async fn list_runs(
        &self,
        flow_id: Option<&str>,
        limit: usize,
    ) -> io::Result<Vec<RunSummary>> {
        let mut runs: Vec<RunSummary> = self
            .read_all_runs()
            .await?
            .iter()
            .filter(|run| flow_id.is_none_or(|id| run.flow_id == id))
            .map(RunRecord::summary)
            .collect();
        runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        runs.truncate(limit);
        Ok(runs)
    }

    /// Uma execucao completa pelo id.
    pub async fn get_run(&self, run_id: &str) -> io::Result<Option<RunRecord>> {
        let Some(path) = self.run_path(run_id) else {
            return Ok(None);
        };
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path).await?;
        serde_json::from_str(&raw)
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    /// Apaga as execucoes mais antigas, mantendo `keep`.
    pub async fn prune_runs(&self, keep: usize) -> io::Result<()> {
        let mut runs = self.read_all_runs().await?;
        if runs.len() <= keep {
            return Ok(());
        }
        runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        for run in runs.into_iter().skip(keep) {
            if let Some(path) = self.run_path(&run.id) {
                let _ = fs::remove_file(path).await;
            }
        }
        Ok(())
    }

    /// Remove todas as execucoes de um fluxo. Chamado quando o fluxo e apagado.
    pub async fn delete_runs_of(&self, flow_id: &str) -> io::Result<()> {
        for run in self.read_all_runs().await? {
            if run.flow_id != flow_id {
                continue;
            }
            if let Some(path) = self.run_path(&run.id) {
                let _ = fs::remove_file(path).await;
            }
        }
        Ok(())
    }

    async fn read_all_runs(&self) -> io::Result<Vec<RunRecord>> {
        if !self.runs_dir.exists() {
            return Ok(Vec::new());
        }
        let mut runs = Vec::new();
        let mut entries = fs::read_dir(&self.runs_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Ok(raw) = fs::read_to_string(&path).await else {
                continue;
            };
            match serde_json::from_str::<RunRecord>(&raw) {
                Ok(run) => runs.push(run),
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "execucao ignorada")
                }
            }
        }
        Ok(runs)
    }

    fn flow_path(&self, id: &str) -> Option<PathBuf> {
        sanitize_id(id).map(|safe| self.flows_dir.join(format!("{safe}.json")))
    }

    fn run_path(&self, id: &str) -> Option<PathBuf> {
        sanitize_id(id).map(|safe| self.runs_dir.join(format!("{safe}.json")))
    }
}

async fn read_flow_file(path: &Path) -> io::Result<Flow> {
    let raw = fs::read_to_string(path).await?;
    serde_json::from_str(&raw).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Escreve JSON em arquivo temporario e renomeia, para nunca deixar um arquivo
/// pela metade se o processo morrer no meio.
async fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let serialized = serde_json::to_vec_pretty(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, serialized).await?;
    match fs::rename(&temp, path).await {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temp).await;
            Err(error)
        }
    }
}

/// Aceita apenas ids que viram nome de arquivo seguro. Barra travessia de
/// diretorio vinda de um id enviado pela API.
fn sanitize_id(id: &str) -> Option<String> {
    let trimmed = id.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return None;
    }
    if !trimmed
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_')
    {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Node;
    use crate::run::{RunStatus, TriggerSource};
    use chrono::Duration;

    fn store() -> (tempfile::TempDir, FlowStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FlowStore::new(dir.path());
        (dir, store)
    }

    fn sample_flow(name: &str) -> Flow {
        let mut flow = Flow::new(name);
        flow.nodes
            .push(Node::new("n1", "Start", "trigger.manual"));
        flow
    }

    fn run_for(flow_id: &str, run_id: &str, minutes_ago: i64) -> RunRecord {
        RunRecord {
            id: run_id.to_string(),
            flow_id: flow_id.to_string(),
            flow_name: "F".to_string(),
            status: RunStatus::Success,
            trigger: TriggerSource::Manual,
            started_at: Utc::now() - Duration::minutes(minutes_ago),
            finished_at: None,
            duration_ms: 1,
            nodes: Vec::new(),
            error: None,
            output: Vec::new(),
        }
    }

    #[tokio::test]
    async fn save_assigns_id_and_timestamps() {
        let (_dir, store) = store();
        let saved = store.save(sample_flow("Primeiro")).await.unwrap();

        assert!(!saved.id.is_empty());
        assert_eq!(saved.schema, FLOW_SCHEMA);
        assert!(saved.created_at.is_some());
        assert!(saved.updated_at.is_some());
    }

    #[tokio::test]
    async fn save_preserves_created_at_on_update() {
        let (_dir, store) = store();
        let first = store.save(sample_flow("Original")).await.unwrap();

        let mut update = first.clone();
        update.name = "Renomeado".to_string();
        let second = store.save(update).await.unwrap();

        assert_eq!(second.id, first.id);
        assert_eq!(second.created_at, first.created_at);
        assert_eq!(second.name, "Renomeado");
        assert!(second.updated_at >= first.updated_at);
    }

    #[tokio::test]
    async fn round_trip_keeps_the_flow_intact() {
        let (_dir, store) = store();
        let saved = store.save(sample_flow("Ida e volta")).await.unwrap();
        let loaded = store.get(&saved.id).await.unwrap().unwrap();
        assert_eq!(loaded, saved);
    }

    #[tokio::test]
    async fn list_is_sorted_by_most_recently_updated() {
        let (_dir, store) = store();
        let first = store.save(sample_flow("Antigo")).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let second = store.save(sample_flow("Novo")).await.unwrap();

        let list = store.list().await.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, second.id);
        assert_eq!(list[1].id, first.id);
    }

    #[tokio::test]
    async fn delete_reports_whether_the_flow_existed() {
        let (_dir, store) = store();
        let saved = store.save(sample_flow("Apagavel")).await.unwrap();

        assert!(store.delete(&saved.id).await.unwrap());
        assert!(!store.delete(&saved.id).await.unwrap());
        assert!(store.get(&saved.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn missing_flow_returns_none_instead_of_error() {
        let (_dir, store) = store();
        assert!(store.get("nao-existe").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn path_traversal_ids_are_refused() {
        let (_dir, store) = store();
        assert!(store.get("../../etc/passwd").await.unwrap().is_none());
        assert!(!store.delete("..\\..\\win.ini").await.unwrap());
        assert_eq!(sanitize_id("a/b"), None);
        assert_eq!(sanitize_id(""), None);
        assert_eq!(sanitize_id("ok-123_ABC"), Some("ok-123_ABC".to_string()));
    }

    #[tokio::test]
    async fn runs_are_listed_newest_first_and_filtered_by_flow() {
        let (_dir, store) = store();
        store.save_run(&run_for("f1", "r1", 10)).await.unwrap();
        store.save_run(&run_for("f1", "r2", 1)).await.unwrap();
        store.save_run(&run_for("f2", "r3", 5)).await.unwrap();

        let all = store.list_runs(None, 10).await.unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].id, "r2");

        let only_f1 = store.list_runs(Some("f1"), 10).await.unwrap();
        assert_eq!(only_f1.len(), 2);
        assert!(only_f1.iter().all(|run| run.flow_id == "f1"));
    }

    #[tokio::test]
    async fn run_history_is_pruned_to_the_limit() {
        let (_dir, store) = store();
        for index in 0..6 {
            store
                .save_run(&run_for("f1", &format!("r{index}"), index))
                .await
                .unwrap();
        }
        store.prune_runs(3).await.unwrap();

        let remaining = store.list_runs(None, 100).await.unwrap();
        assert_eq!(remaining.len(), 3);
        // Mantem as mais recentes, ou seja, as de menor "minutos atras".
        assert_eq!(remaining[0].id, "r0");
    }

    #[tokio::test]
    async fn deleting_a_flow_can_drop_its_runs() {
        let (_dir, store) = store();
        store.save_run(&run_for("f1", "r1", 1)).await.unwrap();
        store.save_run(&run_for("f2", "r2", 1)).await.unwrap();

        store.delete_runs_of("f1").await.unwrap();

        let remaining = store.list_runs(None, 10).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].flow_id, "f2");
    }

    #[tokio::test]
    async fn corrupted_files_are_skipped_not_fatal() {
        let (_dir, store) = store();
        let good = store.save(sample_flow("Bom")).await.unwrap();
        fs::write(store.flows_dir().join("quebrado.json"), "{ nao json")
            .await
            .unwrap();

        let list = store.list().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, good.id);
    }

    #[tokio::test]
    async fn no_temp_file_is_left_behind() {
        let (_dir, store) = store();
        store.save(sample_flow("Atomico")).await.unwrap();

        let mut entries = fs::read_dir(store.flows_dir()).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let name = entry.file_name().to_string_lossy().to_string();
            assert!(!name.ends_with(".tmp"), "sobrou temporario: {name}");
        }
    }
}
