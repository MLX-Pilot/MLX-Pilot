/* Classical script so the deck also opens directly through file://. */
(() => {
  'use strict';
  const slides = Array.from(document.querySelectorAll('.slide'));
  const byId = (id) => document.getElementById(id);
  const previous = byId('previous');
  const next = byId('next');
  const counter = byId('slide-counter');
  const progress = byId('progress');
  const announcer = byId('announcer');
  const notesPanel = byId('notes-panel');
  const notesButton = byId('notes-button');
  const overview = byId('overview');
  const help = byId('help');
  const mediaDialog = byId('media-dialog');
  const motionButton = byId('motion-button');
  const fullscreenButton = byId('fullscreen-button');
  const mediaPreference = window.matchMedia('(prefers-reduced-motion: reduce)');
  const storageKey = 'mlx-pilot-plugins-presentation-reduced-motion';
  // Copied from the shipped VTT files: no fetch is needed on file://.
  const offlineVideoDescriptions = {
  "01-instalar-configurar.mp4": {
    "title": "Instalação e configuração",
    "cues": [
      {
        "start": 0.0,
        "end": 3.8,
        "text": "Abra o pacote Reading Notes e confira as permissões solicitadas."
      },
      {
        "start": 3.8,
        "end": 7.0,
        "text": "Conceda acesso à rede e aos documentos; instale o plugin."
      },
      {
        "start": 7.0,
        "end": 10.5,
        "text": "Em Configurar, autorize o serviço HTTPS da Open Library."
      },
      {
        "start": 10.5,
        "end": 14.5,
        "text": "Selecione um documento e permita criar novas apresentações."
      },
      {
        "start": 14.5,
        "end": 19.266,
        "text": "Salve as autorizações. O plugin está disponível na central."
      }
    ]
  },
  "02-servico-documento.mp4": {
    "title": "Pesquisa real, documento editável",
    "cues": [
      {
        "start": 0.0,
        "end": 3.9,
        "text": "Abra Reading Notes e pesquise livros sobre “urban gardens”."
      },
      {
        "start": 3.9,
        "end": 7.8,
        "text": "O broker consulta a Open Library real e retorna cinco obras."
      },
      {
        "start": 7.8,
        "end": 10.6,
        "text": "Crie um briefing no Studio com os metadados e as fontes."
      },
      {
        "start": 10.6,
        "end": 14.833,
        "text": "Abra o documento criado. Ele pode continuar sendo editado no Studio."
      }
    ]
  },
  "03-studio-exportar.mp4": {
    "title": "Edição pelo mouse e exportação real",
    "cues": [
      {
        "start": 0.0,
        "end": 4.2,
        "text": "Selecione o título com o mouse e altere o texto no inspetor."
      },
      {
        "start": 4.2,
        "end": 6.0,
        "text": "Navegue pelos slides do documento previamente preparado."
      },
      {
        "start": 6.0,
        "end": 11.6,
        "text": "Abra Apresentar para reproduzir as animações do player HTML."
      },
      {
        "start": 11.6,
        "end": 14.8,
        "text": "Exporte um PDF estático com a aparência dos slides."
      },
      {
        "start": 14.8,
        "end": 17.3,
        "text": "Exporte um PPTX com elementos editáveis."
      },
      {
        "start": 17.3,
        "end": 19.9,
        "text": "Exporte também o HTML animado, que abre offline no navegador."
      }
    ]
  }
};
  let current = 0;
  let motionChoice = null;
  let announcementTimer;

  try {
    const stored = window.localStorage.getItem(storageKey);
    if (stored === 'true' || stored === 'false') motionChoice = stored === 'true';
  } catch (_) { /* Some file:// and private browsing contexts deny storage. */ }

  function announce(message) {
    window.clearTimeout(announcementTimer);
    announcer.textContent = '';
    announcementTimer = window.setTimeout(() => { announcer.textContent = message; }, 30);
  }

  function pauseVideos(exceptSlide) {
    slides.forEach((slide) => {
      if (slide !== exceptSlide) slide.querySelectorAll('video').forEach((video) => video.pause());
    });
  }

  function slideFromHash() {
    const match = /^#slide-(\d+)$/.exec(window.location.hash);
    return match ? Math.min(slides.length - 1, Math.max(0, Number(match[1]) - 1)) : 0;
  }

  function showSlide(index, { updateHash = true, announceChange = true } = {}) {
    const target = Math.min(slides.length - 1, Math.max(0, index));
    current = target;
    pauseVideos(slides[current]);
    slides.forEach((slide, slideIndex) => {
      const active = slideIndex === current;
      slide.hidden = !active;
      slide.classList.toggle('is-active', active);
      slide.setAttribute('aria-hidden', String(!active));
      // Native hidden also removes media controls and links from the tab order.
    });
    previous.disabled = current === 0;
    next.disabled = current === slides.length - 1;
    counter.replaceChildren(document.createTextNode(String(current + 1).padStart(2, '0') + ' '));
    const total = document.createElement('span');
    total.textContent = '/ ' + String(slides.length).padStart(2, '0');
    counter.append(total);
    progress.value = current + 1;
    progress.max = slides.length;
    progress.textContent = `${current + 1} de ${slides.length}`;
    progress.setAttribute('aria-valuetext', `Slide ${current + 1} de ${slides.length}`);
    byId('current-slide-title').textContent = slides[current].dataset.title;
    byId('notes-content').textContent = slides[current].querySelector('.slide-notes')?.textContent.trim() || 'Este slide não tem notas.';
    document.querySelectorAll('.overview-choice').forEach((choice, choiceIndex) => {
      if (choiceIndex === current) choice.setAttribute('aria-current', 'true');
      else choice.removeAttribute('aria-current');
    });
    if (updateHash && window.location.hash !== '#' + slides[current].id) {
      try { window.history.replaceState(null, '', '#' + slides[current].id); }
      catch (_) { window.location.hash = slides[current].id; }
    }
    if (announceChange) announce(`Slide ${current + 1} de ${slides.length}: ${slides[current].dataset.title}`);
    if (window.matchMedia('(max-width: 700px) and (orientation: portrait)').matches) window.scrollTo({ top: 0, behavior: 'instant' });
  }

  function toggleNotes(force) {
    const open = force === undefined ? notesPanel.hidden : force;
    notesPanel.hidden = !open;
    document.body.classList.toggle('notes-open', open);
    notesButton.setAttribute('aria-expanded', String(open));
    announce(open ? 'Notas do apresentador abertas.' : 'Notas do apresentador fechadas.');
  }

  function applyMotionPreference() {
    const reduced = motionChoice === null ? mediaPreference.matches : motionChoice;
    document.body.classList.toggle('reduce-motion', reduced);
    document.body.classList.toggle('allow-motion', !reduced);
    motionButton.setAttribute('aria-pressed', String(reduced));
    motionButton.textContent = reduced ? 'Movimento reduzido' : 'Reduzir movimento';
  }

  function toggleMotion() {
    motionChoice = motionButton.getAttribute('aria-pressed') !== 'true';
    try { window.localStorage.setItem(storageKey, String(motionChoice)); } catch (_) { /* Preference remains valid for this session. */ }
    applyMotionPreference();
    announce(motionChoice ? 'Movimento reduzido ativado.' : 'Animações de entrada ativadas.');
  }

  function openDialog(dialog) {
    pauseVideos();
    if (dialog.open) return;
    if (typeof dialog.showModal === 'function') dialog.showModal();
    else dialog.setAttribute('open', '');
    if (dialog === overview) dialog.querySelector('[aria-current="true"]')?.focus();
  }

  function closeDialog(dialog) {
    if (typeof dialog.close === 'function') dialog.close();
    else dialog.removeAttribute('open');
  }

  async function toggleFullscreen() {
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else if (document.documentElement.requestFullscreen) await document.documentElement.requestFullscreen();
      else announce('Tela cheia não disponível neste navegador. Use o modo de tela cheia do navegador.');
    } catch (_) { announce('Não foi possível abrir a tela cheia. Use o modo de tela cheia do navegador.'); }
  }

  slides.forEach((slide, index) => {
    const choice = document.createElement('button');
    choice.type = 'button';
    choice.className = 'overview-choice';
    choice.setAttribute('aria-label', `Ir para o slide ${index + 1}: ${slide.dataset.title}`);
    const number = document.createElement('span');
    number.textContent = String(index + 1).padStart(2, '0');
    const title = document.createElement('strong');
    title.textContent = slide.dataset.title;
    choice.append(number, title);
    choice.addEventListener('click', () => { closeDialog(overview); showSlide(index); byId('slides').focus({ preventScroll: true }); });
    byId('overview-grid').append(choice);
  });

  previous.addEventListener('click', () => showSlide(current - 1));
  next.addEventListener('click', () => showSlide(current + 1));
  notesButton.addEventListener('click', () => toggleNotes());
  byId('close-notes').addEventListener('click', () => { toggleNotes(false); notesButton.focus(); });
  byId('overview-button').addEventListener('click', () => openDialog(overview));
  byId('help-button').addEventListener('click', () => openDialog(help));
  motionButton.addEventListener('click', toggleMotion);
  fullscreenButton.addEventListener('click', toggleFullscreen);
  document.querySelectorAll('[data-close-dialog]').forEach((button) => button.addEventListener('click', () => closeDialog(button.closest('dialog'))));
  [overview, help, mediaDialog].forEach((dialog) => dialog.addEventListener('click', (event) => {
    if (event.target !== dialog) return;
    const bounds = dialog.getBoundingClientRect();
    if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) closeDialog(dialog);
  }));

  document.addEventListener('keydown', (event) => {
    if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return;
    if (document.querySelector('dialog[open]')) {
      if (event.key.toLowerCase() === 'g' && overview.open) { event.preventDefault(); closeDialog(overview); }
      if (event.key === '?' && help.open) { event.preventDefault(); closeDialog(help); }
      return;
    }
    const target = event.target;
    if (target instanceof Element && target.closest('input, textarea, select, video, [contenteditable="true"]')) return;
    const key = event.key.toLowerCase();
    const buttonOrLink = target instanceof Element && target.closest('button, a');
    if ((key === ' ' || key === 'enter') && buttonOrLink) return;
    const action = {
      arrowright: () => showSlide(current + 1), pagedown: () => showSlide(current + 1),
      ' ': () => showSlide(current + (event.shiftKey ? -1 : 1)),
      arrowleft: () => showSlide(current - 1), pageup: () => showSlide(current - 1),
      home: () => showSlide(0), end: () => showSlide(slides.length - 1),
      g: () => openDialog(overview), n: () => toggleNotes(), f: toggleFullscreen,
      '?': () => openDialog(help)
    }[key];
    if (action) { event.preventDefault(); action(); }
  });

  window.addEventListener('hashchange', () => showSlide(slideFromHash(), { updateHash: false }));
  document.addEventListener('visibilitychange', () => { if (document.hidden) pauseVideos(); });
  window.addEventListener('pagehide', () => pauseVideos());
  window.addEventListener('beforeprint', () => pauseVideos());
  document.addEventListener('fullscreenchange', () => {
    const active = Boolean(document.fullscreenElement);
    fullscreenButton.setAttribute('aria-label', active ? 'Sair da tela cheia' : 'Entrar em tela cheia');
    fullscreenButton.setAttribute('aria-pressed', String(active));
  });
  if (mediaPreference.addEventListener) mediaPreference.addEventListener('change', applyMotionPreference);
  else if (mediaPreference.addListener) mediaPreference.addListener(applyMotionPreference);

  document.querySelectorAll('img').forEach((image) => {
    const fallback = () => {
      image.hidden = true;
      if (image.closest('.wordmark')) image.closest('.wordmark').querySelector('.wordmark-fallback').hidden = false;
      else { const message = image.closest('figure')?.querySelector('.media-fallback'); if (message) message.hidden = false; }
    };
    image.addEventListener('error', fallback);
    if (image.complete && image.naturalWidth === 0) fallback();
  });
  document.querySelectorAll('.app-shot').forEach((figure) => {
    const image = figure.querySelector('img');
    const enlarge = document.createElement('button');
    enlarge.type = 'button';
    enlarge.className = 'media-text-button';
    enlarge.textContent = 'Ampliar captura';
    enlarge.setAttribute('aria-haspopup', 'dialog');
    enlarge.addEventListener('click', () => {
      byId('media-title').textContent = 'Configuração no aplicativo';
      const expanded = document.createElement('img');
      expanded.src = image.getAttribute('src');
      expanded.alt = image.alt;
      expanded.className = 'expanded-capture';
      byId('media-content').replaceChildren(expanded);
      openDialog(mediaDialog);
    });
    figure.querySelector('figcaption')?.append(document.createTextNode(' · '), enlarge);
  });
  document.querySelectorAll('[data-image]').forEach((button) => {
    button.addEventListener('click', () => {
      byId('media-title').textContent = 'Credenciais sob controle do usuário';
      const image = document.createElement('img');
      image.src = button.dataset.image;
      image.alt = 'Captura real do campo de senha e da opção de remover uma credencial, sem revelar uma chave.';
      image.className = 'expanded-capture';
      const description = document.createElement('p');
      description.className = 'help-note';
      description.textContent = 'O cadastro acontece na configuração do aplicativo. O SDK recebe apenas o status. O host injeta a credencial na chamada autorizada.';
      byId('media-content').replaceChildren(image, description);
      openDialog(mediaDialog);
    });
  });
  document.querySelectorAll('video').forEach((video) => {
    // External tracks are not default-selected in the HTML. On file:// remove
    // them before playback so native CC cannot initiate a blocked CORS request.
    if (window.location.protocol === 'file:') video.querySelectorAll('track[src]').forEach((track) => track.remove());
    // Printing a video element is inconsistent across browsers. Keep a local
    // poster image in the print layout instead of depending on a decoded frame.
    const poster = document.createElement('img');
    poster.className = 'print-poster';
    poster.src = video.getAttribute('poster');
    poster.alt = video.getAttribute('aria-label');
    video.after(poster);
    const basename = video.querySelector('source').getAttribute('src').split('/').pop();
    const description = offlineVideoDescriptions[basename];
    if (description) {
      const frame = document.createElement('div');
      frame.className = 'video-frame';
      video.before(frame);
      frame.append(video);
      const caption = document.createElement('div');
      caption.className = 'video-caption';
      caption.setAttribute('aria-hidden', 'true');
      const captionText = document.createElement('span');
      caption.append(captionText);
      frame.append(caption);
      const options = document.createElement('div');
      options.className = 'video-options';
      const cc = document.createElement('button');
      cc.type = 'button';
      cc.className = 'media-text-button';
      cc.textContent = 'CC · Legendas';
      cc.setAttribute('aria-label', 'Legendas em português, disponíveis offline');
      cc.setAttribute('aria-pressed', 'true');
      const transcript = document.createElement('button');
      transcript.type = 'button';
      transcript.className = 'media-text-button';
      transcript.textContent = 'Transcrição visual';
      transcript.setAttribute('aria-haspopup', 'dialog');
      options.append(cc, transcript);
      video.closest('figure').append(options);
      let captionsEnabled = true;
      const synchronizeCaptions = () => {
        const cue = description.cues.find((entry) => video.currentTime >= entry.start && video.currentTime < entry.end);
        const nativeCaptionsVisible = Array.from(video.textTracks).some((track) => track.mode === 'showing');
        caption.hidden = !captionsEnabled || !cue || nativeCaptionsVisible;
        captionText.textContent = cue?.text || '';
      };
      cc.addEventListener('click', () => {
        captionsEnabled = !captionsEnabled;
        cc.setAttribute('aria-pressed', String(captionsEnabled));
        synchronizeCaptions();
      });
      transcript.addEventListener('click', () => {
        byId('media-title').textContent = description.title;
        const content = document.createElement('div');
        content.className = 'transcript-content';
        const introduction = document.createElement('p');
        introduction.textContent = 'Descrição das ações visuais. Esta gravação não tem áudio.';
        const sequence = document.createElement('ol');
        description.cues.forEach((cue) => {
          const item = document.createElement('li');
          item.textContent = cue.text;
          sequence.append(item);
        });
        content.append(introduction, sequence);
        byId('media-content').replaceChildren(content);
        openDialog(mediaDialog);
      });
      // File URLs can block external VTT. Runtime tracks use the embedded cues,
      // including the native fullscreen player. The overlay covers other players.
      const configureNativeTracks = () => {
        if (typeof window.VTTCue !== 'function' || typeof video.addTextTrack !== 'function') return;
        Array.from(video.textTracks).forEach((track) => { track.mode = 'disabled'; });
        const native = video.addTextTrack('captions', 'Português (offline)', 'pt-BR');
        description.cues.forEach((cue) => native.addCue(new window.VTTCue(cue.start, cue.end, cue.text)));
        native.mode = 'disabled';
        const updateFullscreenCaptions = () => {
          const nativeFullscreen = document.fullscreenElement === video || video.webkitDisplayingFullscreen;
          native.mode = captionsEnabled && nativeFullscreen ? 'showing' : 'disabled';
        };
        document.addEventListener('fullscreenchange', updateFullscreenCaptions);
        video.addEventListener('webkitbeginfullscreen', updateFullscreenCaptions);
        video.addEventListener('webkitendfullscreen', updateFullscreenCaptions);
        cc.addEventListener('click', updateFullscreenCaptions);
      };
      configureNativeTracks();
      video.textTracks.addEventListener('change', synchronizeCaptions);
      ['timeupdate', 'seeked', 'loadedmetadata', 'play', 'ended'].forEach((name) => video.addEventListener(name, synchronizeCaptions));
      synchronizeCaptions();
    }
    const fallback = () => {
      const message = video.closest('figure')?.querySelector('.media-fallback');
      if (message) message.hidden = false;
    };
    video.addEventListener('error', fallback);
    video.querySelectorAll('source').forEach((source) => source.addEventListener('error', fallback));
    video.addEventListener('play', () => {
      if (!slides[current].contains(video)) video.pause();
      else document.querySelectorAll('video').forEach((other) => { if (other !== video) other.pause(); });
    });
  });

  applyMotionPreference();
  showSlide(slideFromHash(), { announceChange: false });
})();
