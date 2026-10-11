use extism_pdk::*;
use mlx_presentation::{Document,apply};
use serde::Deserialize;
use serde_json::{Value,json};
#[derive(Deserialize)] struct Edit { document:Document, operations:Vec<Value> }
#[plugin_fn]
pub fn edit(Json(input):Json<Edit>)->FnResult<Json<Value>> {
    let doc=apply(&input.document,&input.operations).map_err(|e|Error::msg(e))?;
    Ok(Json(json!({"document":doc})))
}
