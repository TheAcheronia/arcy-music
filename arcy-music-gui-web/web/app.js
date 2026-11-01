// Arcy Music - Web GUI Application Logic
(function () {
  'use strict';

  const API_BASE = window.location.origin;

  // State
  let currentState = {
    is_playing: false,
    current_track: null,
    position_secs: 0,
    duration_secs: 0,
    volume: 0.15,
    shuffle: false,
    repeat_mode: 'Off',
    queue: [],
    queue_index: 0,
  };

  let playlists = [];
  let currentPlaylist = null;
  let currentPlaylistAllTracks = [];
  let currentPlaylistTracks = [];
  let playlistSearchQuery = '';
  let playlistSortMode = 'custom';
  let playlistSortAsc = true;
  let currentLyrics = { plain: '', synced: [] };
  let currentTrackId = null;
  let isSeeking = false;
  let currentSearchSource = 'YouTube';

  // Helper formatting
  function formatTime(seconds) {
    if (!seconds || isNaN(seconds) || seconds < 0) return '0:00';
    const m = Math.floor(seconds / 60);
    const s = Math.floor(seconds % 60);
    return `${m}:${s < 10 ? '0' : ''}${s}`;
  }

  // Helper: Format thumbnail URL for local and remote images
  function getThumbnailUrl(path) {
    if (!path) return '';
    if (path.startsWith('http://') || path.startsWith('https://')) return path;
    const filename = path.split('/').pop();
    return `/thumbnails/${encodeURIComponent(filename)}`;
  }

  // 1. Navigation & Tabs
  const navButtons = document.querySelectorAll('.nav-item');
  const tabPanes = document.querySelectorAll('.tab-pane');
  const btnPlayerLyrics = document.getElementById('btn-player-lyrics');
  let previousActiveTab = 'library';

  function switchTab(targetTab) {
    const currentActive = document.querySelector('.nav-item.active')?.getAttribute('data-tab');
    if (currentActive && currentActive !== 'lyrics') {
      previousActiveTab = currentActive;
    }

    navButtons.forEach(b => b.classList.remove('active'));
    tabPanes.forEach(p => p.classList.remove('active'));

    const btn = document.querySelector(`.nav-item[data-tab='${targetTab}']`);
    if (btn) btn.classList.add('active');
    const targetPane = document.getElementById(`tab-${targetTab}`);
    if (targetPane) targetPane.classList.add('active');

    const btnPlayerQueue = document.getElementById('btn-player-queue');
    if (btnPlayerQueue) {
      if (targetTab === 'queue') {
        btnPlayerQueue.classList.add('active');
      } else {
        btnPlayerQueue.classList.remove('active');
      }
    }

    if (btnPlayerLyrics) {
      if (targetTab === 'lyrics') {
        btnPlayerLyrics.classList.add('active');
      } else {
        btnPlayerLyrics.classList.remove('active');
      }
    }

    if (targetTab === 'queue') renderQueue();
    if (targetTab === 'equalizer') loadEqConfig();
    if (targetTab === 'settings') {
      loadAudioDevices();
      loadDownloadPath();
    }
    if (targetTab === 'lyrics' && currentState.current_track) {
      loadLyrics(currentState.current_track.id);
      updateLyricsAmbientBackdrop();
    }
  }

  navButtons.forEach(btn => {
    btn.addEventListener('click', () => {
      const targetTab = btn.getAttribute('data-tab');
      const currentActive = document.querySelector('.nav-item.active')?.getAttribute('data-tab');
      if (targetTab === 'library' && currentActive === 'library') {
        const backBtn = document.getElementById('btn-back-to-playlists');
        const playlistDetail = document.getElementById('subview-playlist-detail');
        if (backBtn && playlistDetail && !playlistDetail.classList.contains('hidden')) {
          backBtn.click();
          return;
        }
      }
      switchTab(targetTab);
    });
  });

  // Toggle queue from bottom playback bar
  document.getElementById('btn-player-queue')?.addEventListener('click', () => {
    const currentActive = document.querySelector('.nav-item.active')?.getAttribute('data-tab');
    if (currentActive === 'queue') {
      switchTab(previousActiveTab || 'library');
    } else {
      previousActiveTab = currentActive || 'library';
      switchTab('queue');
    }
  });

  // Toggle lyrics from bottom playback bar
  btnPlayerLyrics?.addEventListener('click', () => {
    const currentActive = document.querySelector('.nav-item.active')?.getAttribute('data-tab');
    if (currentActive === 'lyrics') {
      switchTab(previousActiveTab || 'library');
    } else {
      switchTab('lyrics');
    }
  });

  const hash = window.location.hash.replace('#', '');
  if (hash) {
    switchTab(hash);
  }

  // 2. GUI Switchers & Exits
  async function switchToNative() {
    try {
      await fetch(`${API_BASE}/api/gui/switch-to-native`, { method: 'POST' });
    } catch (e) {
      console.error('Error switching to native GUI:', e);
    }
    window.close();
  }

  document.getElementById('btn-switch-to-native')?.addEventListener('click', switchToNative);
  document.getElementById('btn-settings-switch-native')?.addEventListener('click', switchToNative);

  document.getElementById('btn-exit-web-gui')?.addEventListener('click', () => {
    window.close();
  });

  document.getElementById('btn-stop-daemon')?.addEventListener('click', async () => {
    if (confirm('Stop the Arcy Music Daemon and exit completely?')) {
      try {
        await fetch(`${API_BASE}/api/daemon/shutdown`, { method: 'POST' });
      } catch (e) {}
      window.close();
    }
  });

  // 3. Playback Controls
  async function sendCommand(cmd) {
    try {
      await fetch(`${API_BASE}/api/command`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(cmd),
      });
      fetchState();
    } catch (e) {
      console.error('Failed to send command:', e);
    }
  }

  document.getElementById('btn-play-pause')?.addEventListener('click', () => sendCommand('PlayPause'));
  document.getElementById('btn-next')?.addEventListener('click', () => sendCommand('Next'));
  document.getElementById('btn-prev')?.addEventListener('click', () => sendCommand('Previous'));
  document.getElementById('btn-shuffle')?.addEventListener('click', () => sendCommand('ToggleShuffle'));
  document.getElementById('btn-repeat')?.addEventListener('click', () => sendCommand('ToggleRepeat'));

  // Seeker
  const seeker = document.getElementById('player-seeker');
  seeker?.addEventListener('mousedown', () => { isSeeking = true; });
  seeker?.addEventListener('touchstart', () => { isSeeking = true; });
  seeker?.addEventListener('change', () => {
    isSeeking = false;
    sendCommand({ Seek: parseFloat(seeker.value) });
  });

  // Volume & Mute Controls
  const volumeSlider = document.getElementById('volume-slider');
  const volumeText = document.getElementById('volume-text');
  const btnMute = document.getElementById('btn-mute');
  let isChangingVolume = false;
  let previousVolume = 0.5;

  function updateMuteIcon(vol) {
    if (!btnMute) return;
    if (vol <= 0.001) {
      btnMute.innerHTML = '<i class="fa-solid fa-volume-xmark"></i>';
    } else if (vol < 0.5) {
      btnMute.innerHTML = '<i class="fa-solid fa-volume-low"></i>';
    } else {
      btnMute.innerHTML = '<i class="fa-solid fa-volume-high"></i>';
    }
  }

  function toggleMute() {
    if (currentState.volume > 0.001) {
      previousVolume = currentState.volume;
      sendCommand({ SetVolume: 0.0 });
      if (volumeSlider) volumeSlider.value = 0.0;
      if (volumeText) volumeText.textContent = '0%';
      updateMuteIcon(0.0);
    } else {
      const restoreVol = previousVolume > 0.05 ? previousVolume : 0.5;
      sendCommand({ SetVolume: restoreVol });
      if (volumeSlider) volumeSlider.value = restoreVol;
      if (volumeText) volumeText.textContent = `${Math.round(restoreVol * 100)}%`;
      updateMuteIcon(restoreVol);
    }
  }

  btnMute?.addEventListener('click', toggleMute);

  volumeSlider?.addEventListener('mousedown', () => { isChangingVolume = true; });
  volumeSlider?.addEventListener('touchstart', () => { isChangingVolume = true; });
  volumeSlider?.addEventListener('mouseup', () => { isChangingVolume = false; });
  volumeSlider?.addEventListener('touchend', () => { isChangingVolume = false; });

  volumeSlider?.addEventListener('input', (e) => {
    const vol = parseFloat(e.target.value);
    volumeText.textContent = `${Math.round(vol * 100)}%`;
    updateMuteIcon(vol);
    sendCommand({ SetVolume: vol });
  });

  volumeSlider?.addEventListener('change', (e) => {
    isChangingVolume = false;
    const vol = parseFloat(e.target.value);
    sendCommand({ SetVolume: vol });
  });

  // Floating Toast Notification
  function showToast(msg) {
    let container = document.getElementById('toast-container');
    if (!container) {
      container = document.createElement('div');
      container.id = 'toast-container';
      document.body.appendChild(container);
    }
    const toast = document.createElement('div');
    toast.className = 'toast-message';
    toast.textContent = msg;
    container.appendChild(toast);
    setTimeout(() => toast.remove(), 3000);
  }

  // Debounced Track Click Handler (prevents rapid double-click restarts)
  let lastTrackClickTime = 0;
  let lastTrackClickId = null;
  function handleTrackClick(trackId, trackObj) {
    const now = Date.now();
    const id = trackId || trackObj?.id;
    if (id && lastTrackClickId === id && (now - lastTrackClickTime) < 800) {
      console.log('Debouncing rapid click on track:', id);
      return;
    }
    lastTrackClickTime = now;
    lastTrackClickId = id;

    if (trackId) {
      sendCommand({ PlayTrackId: trackId });
    } else if (trackObj) {
      sendCommand({ PlayTrack: trackObj });
    }
  }

  // 4. State Polling & Synchronization
  async function fetchState() {
    try {
      const res = await fetch(`${API_BASE}/api/state`);
      if (!res.ok) return;
      const state = await res.json();
      currentState = state;
      updateUI();
    } catch (e) {}
  }

  function updateUI() {
    // Play/Pause icon
    const playBtn = document.getElementById('btn-play-pause');
    if (playBtn) {
      playBtn.innerHTML = currentState.is_playing
        ? '<i class="fa-solid fa-pause"></i>'
        : '<i class="fa-solid fa-play"></i>';
    }

    // Now Playing Banner & Left Meta
    const headerBanner = document.getElementById('header-now-playing-text');
    const playerTitle = document.getElementById('player-title');
    const playerArtist = document.getElementById('player-artist');
    const coverBox = document.getElementById('player-cover-box');
    const coverImg = document.getElementById('player-cover-img');

    if (currentState.current_track) {
      const t = currentState.current_track;
      if (headerBanner) headerBanner.textContent = `${t.title} - ${t.artist || 'Unknown'}`;
      if (playerTitle) playerTitle.textContent = t.title;
      if (playerArtist) playerArtist.textContent = t.artist || 'Unknown Artist';

      const thumbUrl = getThumbnailUrl(t.thumbnail_url);
      if (thumbUrl && coverImg) {
        coverImg.src = thumbUrl;
        coverImg.classList.remove('hidden');
        if (coverBox?.querySelector('.placeholder-icon')) {
          coverBox.querySelector('.placeholder-icon').classList.add('hidden');
        }
      } else if (coverImg) {
        coverImg.classList.add('hidden');
        if (coverBox?.querySelector('.placeholder-icon')) {
          coverBox.querySelector('.placeholder-icon').classList.remove('hidden');
        }
      }

      // Check track change for lyrics
      if (t.id !== currentTrackId) {
        currentTrackId = t.id;
        loadLyrics(t.id);
        updateLyricsAmbientBackdrop();
      }
    } else {
      if (headerBanner) headerBanner.textContent = 'No track playing';
      if (playerTitle) playerTitle.textContent = 'No track playing';
      if (playerArtist) playerArtist.textContent = 'Select a song from library';
    }

    // Seeker & Time
    const posTime = document.getElementById('player-pos-time');
    const durTime = document.getElementById('player-dur-time');
    if (posTime) posTime.textContent = formatTime(currentState.position_secs);
    if (durTime) durTime.textContent = formatTime(currentState.duration_secs);

    if (seeker && !isSeeking) {
      seeker.max = currentState.duration_secs > 0 ? currentState.duration_secs : 100;
      seeker.value = currentState.position_secs;
    }

    // Volume & Mute synchronization
    if (volumeSlider && !isChangingVolume && currentState.volume !== undefined) {
      volumeSlider.value = currentState.volume;
      if (volumeText) {
        volumeText.textContent = `${Math.round(currentState.volume * 100)}%`;
      }
      updateMuteIcon(currentState.volume);
    }

    // Controls active state
    const btnShuffle = document.getElementById('btn-shuffle');
    if (btnShuffle) {
      if (currentState.shuffle) btnShuffle.classList.add('active');
      else btnShuffle.classList.remove('active');
    }

    const btnRepeat = document.getElementById('btn-repeat');
    if (btnRepeat) {
      if (currentState.repeat_mode !== 'Off') btnRepeat.classList.add('active');
      else btnRepeat.classList.remove('active');
    }

    // Sync active lyric line
    syncActiveLyric(currentState.position_secs);

    // Sync highlight in tracks list
    document.querySelectorAll('.track-row').forEach(row => {
      if (currentState.current_track && row.getAttribute('data-id') === currentState.current_track.id) {
        row.classList.add('playing');
      } else {
        row.classList.remove('playing');
      }
    });

    // Update queue if queue tab is active
    const queuePane = document.getElementById('tab-queue');
    if (queuePane && queuePane.classList.contains('active')) {
      renderQueue();
    }

    // Update queue button badge/title
    const btnPlayerQueue = document.getElementById('btn-player-queue');
    if (btnPlayerQueue) {
      const qLen = currentState.queue ? currentState.queue.length : 0;
      btnPlayerQueue.title = `Playback Queue (${qLen})`;
    }
  }

  // 5. Playlists & Library
  async function loadPlaylists() {
    try {
      const res = await fetch(`${API_BASE}/api/playlists`);
      if (!res.ok) return;
      playlists = await res.json();
      renderPlaylistsGrid();
    } catch (e) {
      console.error('Failed to load playlists:', e);
    }
  }

  let playlistFilterQuery = '';
  const playlistFilterInput = document.getElementById('playlist-filter-input');
  const btnClearPlFilter = document.getElementById('btn-clear-playlist-filter');

  function renderPlaylistsGrid() {
    const grid = document.getElementById('playlists-grid');
    if (!grid) return;
    grid.innerHTML = '';

    const q = playlistFilterQuery.trim().toLowerCase();
    const filtered = q
      ? playlists.filter(pl => pl.name && pl.name.toLowerCase().includes(q))
      : playlists;

    if (filtered.length === 0) {
      grid.innerHTML = `<div class="search-status" style="grid-column: 1/-1;">No playlists matching "${playlistFilterQuery}".</div>`;
      return;
    }

    filtered.forEach(pl => {
      const card = document.createElement('div');
      card.className = 'playlist-card';
      card.innerHTML = `
        <div class="card-top">
          <i class="fa-solid fa-list-music card-icon"></i>
          ${pl.is_pinned ? '<span class="badge-default">PINNED</span>' : ''}
          ${pl.id === 'default' ? '<span class="badge-default">DEFAULT</span>' : ''}
        </div>
        <div class="card-title">${pl.name}</div>
        <div class="card-count">${pl.track_count || 0} tracks</div>
      `;
      card.addEventListener('click', () => openPlaylistDetail(pl));
      grid.appendChild(card);
    });
  }

  playlistFilterInput?.addEventListener('input', (e) => {
    playlistFilterQuery = e.target.value;
    if (playlistFilterQuery) {
      btnClearPlFilter?.classList.remove('hidden');
    } else {
      btnClearPlFilter?.classList.add('hidden');
    }
    renderPlaylistsGrid();
  });

  btnClearPlFilter?.addEventListener('click', () => {
    playlistFilterQuery = '';
    if (playlistFilterInput) playlistFilterInput.value = '';
    btnClearPlFilter.classList.add('hidden');
    renderPlaylistsGrid();
  });

  async function openPlaylistDetail(pl) {
    currentPlaylist = pl;
    document.getElementById('subview-playlists')?.classList.add('hidden');
    const detailView = document.getElementById('subview-playlist-detail');
    if (detailView) detailView.classList.remove('hidden');

    document.getElementById('detail-playlist-title').textContent = pl.name;
    const badge = document.getElementById('detail-playlist-badge');
    if (badge) badge.style.display = pl.id === 'default' ? 'inline-block' : 'none';

    try {
      const res = await fetch(`${API_BASE}/api/playlists/${pl.id}/tracks`);
      if (res.ok) {
        currentPlaylistAllTracks = await res.json();
        playlistSearchQuery = '';
        const searchInput = document.getElementById('playlist-search-input');
        if (searchInput) searchInput.value = '';
        document.getElementById('btn-clear-playlist-search')?.classList.add('hidden');
        filterAndSortPlaylistTracks();
      }
    } catch (e) {
      console.error('Failed to load playlist tracks:', e);
    }
  }

  document.getElementById('btn-back-to-playlists')?.addEventListener('click', () => {
    document.getElementById('subview-playlist-detail')?.classList.add('hidden');
    document.getElementById('subview-playlists')?.classList.remove('hidden');
    currentPlaylist = null;
    currentPlaylistAllTracks = [];
    currentPlaylistTracks = [];
    playlistSearchQuery = '';
    const searchInput = document.getElementById('playlist-search-input');
    if (searchInput) searchInput.value = '';
    loadPlaylists();
  });

  // Track search inside playlist
  const plSearchInput = document.getElementById('playlist-search-input');
  const btnClearPlSearch = document.getElementById('btn-clear-playlist-search');

  plSearchInput?.addEventListener('input', (e) => {
    playlistSearchQuery = e.target.value;
    if (playlistSearchQuery) {
      btnClearPlSearch?.classList.remove('hidden');
    } else {
      btnClearPlSearch?.classList.add('hidden');
    }
    filterAndSortPlaylistTracks();
  });

  btnClearPlSearch?.addEventListener('click', () => {
    if (plSearchInput) plSearchInput.value = '';
    playlistSearchQuery = '';
    btnClearPlSearch?.classList.add('hidden');
    filterAndSortPlaylistTracks();
    plSearchInput?.focus();
  });

  // Sorting
  document.querySelectorAll('.sort-pill').forEach(pill => {
    pill.addEventListener('click', () => {
      const sortType = pill.getAttribute('data-sort');
      document.querySelectorAll('.sort-pill').forEach(p => p.classList.remove('active'));
      pill.classList.add('active');

      if (playlistSortMode === sortType) {
        playlistSortAsc = !playlistSortAsc;
      } else {
        playlistSortMode = sortType;
        playlistSortAsc = true;
      }

      filterAndSortPlaylistTracks();
    });
  });

  function filterAndSortPlaylistTracks() {
    const q = playlistSearchQuery.trim().toLowerCase();
    if (!q) {
      currentPlaylistTracks = [...currentPlaylistAllTracks];
    } else {
      currentPlaylistTracks = currentPlaylistAllTracks.filter(t => {
        const titleMatch = t.title && t.title.toLowerCase().includes(q);
        const artistMatch = t.artist && t.artist.toLowerCase().includes(q);
        const albumMatch = t.album && t.album.toLowerCase().includes(q);
        return titleMatch || artistMatch || albumMatch;
      });
    }

    const countElem = document.getElementById('detail-playlist-count');
    if (countElem) {
      if (q) {
        countElem.textContent = `(${currentPlaylistTracks.length} of ${currentPlaylistAllTracks.length} tracks)`;
      } else {
        countElem.textContent = `(${currentPlaylistAllTracks.length} tracks)`;
      }
    }

    sortTracksList();
    renderPlaylistTracks();
  }

  function sortTracksList() {
    if (playlistSortMode === 'title') {
      currentPlaylistTracks.sort((a, b) => {
        const res = a.title.localeCompare(b.title);
        return playlistSortAsc ? res : -res;
      });
    } else if (playlistSortMode === 'artist') {
      currentPlaylistTracks.sort((a, b) => {
        const artA = a.artist || '';
        const artB = b.artist || '';
        const res = artA.localeCompare(artB);
        return playlistSortAsc ? res : -res;
      });
    } else if (playlistSortMode === 'duration') {
      currentPlaylistTracks.sort((a, b) => {
        const durA = a.duration_secs || 0;
        const durB = b.duration_secs || 0;
        return playlistSortAsc ? durA - durB : durB - durA;
      });
    }
  }

  function renderPlaylistTracks() {
    const list = document.getElementById('playlist-tracks-list');
    if (!list) return;
    list.innerHTML = '';

    if (currentPlaylistTracks.length === 0) {
      list.innerHTML = playlistSearchQuery
        ? '<div class="search-status">No tracks matching "' + playlistSearchQuery + '" in this playlist.</div>'
        : '<div class="search-status">Playlist is empty.</div>';
      return;
    }

    currentPlaylistTracks.forEach((t, idx) => {
      const row = document.createElement('div');
      row.className = `track-row ${currentState.current_track && currentState.current_track.id === t.id ? 'playing' : ''}`;
      row.setAttribute('data-id', t.id);

      const thumbSrc = getThumbnailUrl(t.thumbnail_url);
      const thumbHtml = thumbSrc
        ? `<img src="${thumbSrc}" class="track-thumb" alt="cover" onerror="this.outerHTML='<div class=\\'track-thumb\\'><i class=\\'fa-solid fa-music\\'></i></div>'">`
        : `<div class="track-thumb"><i class="fa-solid fa-music"></i></div>`;

      row.innerHTML = `
        <div class="col-idx">${idx + 1}</div>
        <div class="col-cover">${thumbHtml}</div>
        <div class="col-title track-title-text">${t.title}</div>
        <div class="col-artist track-artist-text">${t.artist || 'Unknown'}</div>
        <div class="col-duration track-dur-text">${formatTime(t.duration_secs)}</div>
        <div class="col-actions">
          <button class="track-action-btn btn-enqueue" title="Add to Queue"><i class="fa-solid fa-plus"></i></button>
          <button class="track-action-btn btn-download" title="Download MP3"><i class="fa-solid fa-arrow-down-to-line"></i></button>
        </div>
      `;

      row.addEventListener('click', (e) => {
        if (e.target.closest('.btn-enqueue')) {
          e.stopPropagation();
          sendCommand({ EnqueueTrack: t });
          showToast(`Added to queue: ${t.title}`);
          return;
        }
        if (e.target.closest('.btn-download')) {
          e.stopPropagation();
          sendCommand({ DownloadTrack: t });
          showToast(`Downloading: ${t.title}`);
          return;
        }
        handleTrackClick(t.id, t);
      });

      list.appendChild(row);
    });
  }

  document.getElementById('btn-play-all-playlist')?.addEventListener('click', () => {
    if (currentPlaylistTracks.length > 0) {
      sendCommand({ PlayTrack: currentPlaylistTracks[0] });
    }
  });

  // 6. Online Search
  document.querySelectorAll('.source-pill').forEach(pill => {
    pill.addEventListener('click', () => {
      document.querySelectorAll('.source-pill').forEach(p => p.classList.remove('active'));
      pill.classList.add('active');
      currentSearchSource = pill.getAttribute('data-source');
    });
  });

  const searchInput = document.getElementById('search-input');
  const btnSearch = document.getElementById('btn-perform-search');

  async function performSearch() {
    const query = searchInput.value.trim();
    if (!query) return;

    const statusBar = document.getElementById('search-status-bar');
    const resultsList = document.getElementById('search-results-list');
    statusBar.classList.remove('hidden');
    statusBar.textContent = `Searching ${currentSearchSource} for "${query}"...`;
    resultsList.innerHTML = '';

    try {
      const endpoint = currentSearchSource === 'Spotify'
        ? `${API_BASE}/api/search/spotify/${encodeURIComponent(query)}`
        : `${API_BASE}/api/search/youtube/${encodeURIComponent(query)}`;

      const res = await fetch(endpoint);
      if (!res.ok) throw new Error('Search failed');
      const results = await res.json();

      statusBar.classList.add('hidden');
      if (results.length === 0) {
        resultsList.innerHTML = '<div class="search-status">No tracks found.</div>';
        return;
      }

      results.forEach((t, idx) => {
        const row = document.createElement('div');
        row.className = 'track-row';
        const thumbSrc = getThumbnailUrl(t.thumbnail_url);
        const thumbHtml = thumbSrc
          ? `<img src="${thumbSrc}" class="track-thumb" alt="cover" onerror="this.outerHTML='<div class=\\'track-thumb\\'><i class=\\'fa-solid fa-music\\'></i></div>'">`
          : `<div class="track-thumb"><i class="fa-solid fa-music"></i></div>`;

        row.innerHTML = `
          <div class="col-idx">${idx + 1}</div>
          <div class="col-cover">${thumbHtml}</div>
          <div class="col-title track-title-text">${t.title}</div>
          <div class="col-artist track-artist-text">${t.artist || 'Unknown'}</div>
          <div class="col-duration track-dur-text">${formatTime(t.duration_secs)}</div>
          <div class="col-actions">
            <button class="track-action-btn btn-enqueue" title="Add to Queue"><i class="fa-solid fa-plus"></i></button>
            <button class="track-action-btn btn-download" title="Download MP3"><i class="fa-solid fa-arrow-down-to-line"></i></button>
          </div>
        `;

        row.addEventListener('click', (e) => {
          if (e.target.closest('.btn-enqueue')) {
            e.stopPropagation();
            sendCommand({ EnqueueTrack: t });
            showToast(`Added to queue: ${t.title}`);
            return;
          }
          if (e.target.closest('.btn-download')) {
            e.stopPropagation();
            sendCommand({ DownloadTrack: t });
            showToast(`Downloading: ${t.title}`);
            return;
          }
          handleTrackClick(null, t);
        });

        resultsList.appendChild(row);
      });
    } catch (e) {
      statusBar.textContent = `Search error: ${e.message}`;
    }
  }

  btnSearch?.addEventListener('click', performSearch);
  searchInput?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') performSearch();
  });

  // 6.5. Playback Queue
  let lastQueueRenderSignature = '';

  function renderQueue(force = false) {
    const list = document.getElementById('queue-tracks-list');
    const countLabel = document.getElementById('queue-count-label');
    if (!list) return;

    const queue = currentState.queue || [];
    const currentIndex = currentState.queue_index ?? -1;

    if (countLabel) {
      countLabel.textContent = `${queue.length} track${queue.length === 1 ? '' : 's'} in queue`;
    }

    const signature = `${currentIndex}:${queue.map(t => `${t.id}:${t.title}`).join('|')}`;
    if (!force && signature === lastQueueRenderSignature) {
      return;
    }
    lastQueueRenderSignature = signature;

    if (queue.length === 0) {
      list.innerHTML = `
        <div class="queue-empty-state">
          <i class="fa-solid fa-list-check"></i>
          <h3>Your Queue is Empty</h3>
          <p>Add tracks to the queue from your Library playlists or Search to keep the music playing smoothly.</p>
        </div>
      `;
      return;
    }

    list.innerHTML = '';
    queue.forEach((t, idx) => {
      const isCurrent = idx === currentIndex;
      const item = document.createElement('div');
      item.className = `queue-item ${isCurrent ? 'active' : ''}`;
      item.setAttribute('data-index', idx);

      const thumbSrc = getThumbnailUrl(t.thumbnail_url);
      const thumbHtml = thumbSrc
        ? `<img src="${thumbSrc}" alt="cover" onerror="this.outerHTML='<i class=\\'fa-solid fa-music\\'></i>'">`
        : `<i class="fa-solid fa-music"></i>`;

      item.innerHTML = `
        <div class="queue-index">${isCurrent ? '<i class="fa-solid fa-volume-high"></i>' : idx + 1}</div>
        <div class="queue-thumb">${thumbHtml}</div>
        <div class="queue-meta">
          <div class="queue-title">${t.title || 'Unknown Title'}</div>
          <div class="queue-artist">${t.artist || 'Unknown Artist'}</div>
        </div>
        <div class="queue-duration">${formatTime(t.duration_secs)}</div>
        <div class="queue-actions">
          <button class="track-action-btn btn-remove-queue" title="Remove from Queue" data-index="${idx}">
            <i class="fa-solid fa-xmark"></i>
          </button>
        </div>
      `;

      item.addEventListener('click', (e) => {
        if (e.target.closest('.btn-remove-queue')) {
          e.stopPropagation();
          sendCommand({ RemoveFromQueue: idx });
          return;
        }
        sendCommand({ SetQueueAndPlay: { tracks: currentState.queue, start_index: idx } });
      });

      list.appendChild(item);
    });
  }

  document.getElementById('btn-clear-queue')?.addEventListener('click', () => {
    sendCommand('ClearQueue');
  });

  // 7. DSP Equalizer & Spatial Audio
  let isEqActive = true;
  let currentSpatialMode = 'off';

  function updateEqToggleButton() {
    const btn = document.getElementById('btn-toggle-eq');
    if (!btn) return;
    if (isEqActive) {
      btn.textContent = 'EQ ACTIVE';
      btn.className = 'btn-accent';
    } else {
      btn.textContent = 'EQ BYPASS';
      btn.className = 'btn-secondary';
    }
  }

  function updateSpatialModeUI() {
    const badge = document.getElementById('spatial-badge');
    if (badge) {
      if (currentSpatialMode === 'dolby_atmos') {
        badge.textContent = 'Dolby Atmos';
        badge.style.borderColor = 'var(--accent)';
        badge.style.color = 'var(--accent)';
      } else if (currentSpatialMode === 'eclipsa') {
        badge.textContent = 'Eclipsa Audio';
        badge.style.borderColor = 'var(--accent)';
        badge.style.color = 'var(--accent)';
      } else {
        badge.textContent = 'Bypass';
        badge.style.borderColor = '';
        badge.style.color = '';
      }
    }

    document.querySelectorAll('.spatial-chip').forEach(chip => {
      if (chip.getAttribute('data-spatial') === currentSpatialMode) {
        chip.classList.add('active');
      } else {
        chip.classList.remove('active');
      }
    });
  }

  async function loadEqConfig() {
    try {
      const res = await fetch(`${API_BASE}/api/dsp/eq`);
      if (!res.ok) return;
      const eq = await res.json();
      isEqActive = eq.enabled !== false;
      currentSpatialMode = eq.spatial_mode || 'off';
      updateEqToggleButton();
      updateSpatialModeUI();
      for (let i = 0; i < 5; i++) {
        const slider = document.getElementById(`eq-band-${i}`);
        const valSpan = document.getElementById(`eq-val-${i}`);
        if (slider && valSpan) {
          slider.value = eq.bands ? (eq.bands[i] || 0) : 0;
          valSpan.textContent = `${slider.value > 0 ? '+' : ''}${slider.value} dB`;
        }
      }
    } catch (e) {}
  }

  document.getElementById('btn-toggle-eq')?.addEventListener('click', () => {
    isEqActive = !isEqActive;
    updateEqToggleButton();
    saveEqConfig();
  });

  // Spatial chips listener
  document.querySelectorAll('.spatial-chip').forEach(chip => {
    chip.addEventListener('click', () => {
      currentSpatialMode = chip.getAttribute('data-spatial') || 'off';
      updateSpatialModeUI();
      saveEqConfig();
    });
  });

  const eqPresets = {
    flat: [0, 0, 0, 0, 0],
    bass_boost: [5, 3.5, 0, 0.5, 1],
    vocal: [-1, 0.5, 3.5, 4, 1.5],
    electronic: [5.0, 3.0, -1.0, 2.0, 4.0],
    rock: [4.0, 2.0, -2.0, 3.0, 4.0],
  };

  document.querySelectorAll('.eq-presets button').forEach((btn) => {
    btn.addEventListener('click', () => {
      const p = btn.dataset.preset;
      const bands = eqPresets[p];
      if (bands) {
        for (let i = 0; i < 5; i++) {
          const slider = document.getElementById(`eq-band-${i}`);
          const valSpan = document.getElementById(`eq-val-${i}`);
          if (slider && valSpan) {
            slider.value = bands[i];
            valSpan.textContent = `${bands[i] > 0 ? '+' : ''}${bands[i]} dB`;
          }
        }
        saveEqConfig();
      }
    });
  });

  for (let i = 0; i < 5; i++) {
    const slider = document.getElementById(`eq-band-${i}`);
    slider?.addEventListener('input', (e) => {
      const val = parseFloat(e.target.value);
      const valSpan = document.getElementById(`eq-val-${i}`);
      if (valSpan) valSpan.textContent = `${val > 0 ? '+' : ''}${val} dB`;
      saveEqConfig();
    });
  }

  async function saveEqConfig() {
    const bands = [];
    for (let i = 0; i < 5; i++) {
      bands.push(parseFloat(document.getElementById(`eq-band-${i}`)?.value || 0));
    }
    try {
      await fetch(`${API_BASE}/api/dsp/eq`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          enabled: isEqActive,
          preamp_db: 0.0,
          bands,
          spatial_mode: currentSpatialMode,
        }),
      });
    } catch (e) {}
  }

  // 8. Synchronized Lyrics
  let lastActiveLyricIdx = -1;
  let isUserScrolling = false;
  let userScrollTimeout = null;
  let isProgrammaticScroll = false;
  let programmaticScrollTimeout = null;
  let isUserDraggingScrollbar = false;

  function scrollToLyric(element, smooth = true) {
    const container = document.getElementById('lyrics-container');
    if (!element || !container) return;
    isProgrammaticScroll = true;
    if (programmaticScrollTimeout) clearTimeout(programmaticScrollTimeout);

    element.scrollIntoView({ behavior: smooth ? 'smooth' : 'auto', block: 'center' });

    programmaticScrollTimeout = setTimeout(() => {
      isProgrammaticScroll = false;
    }, 800);
  }

  function updateLyricsAmbientBackdrop() {
    const ambientEl = document.getElementById('lyrics-ambient-bg');
    if (!ambientEl) return;
    if (currentState.current_track?.thumbnail_url) {
      const url = getThumbnailUrl(currentState.current_track.thumbnail_url);
      if (url) {
        ambientEl.style.backgroundImage = `url('${url}')`;
        ambientEl.style.opacity = '0.35';
        return;
      }
    }
    ambientEl.style.backgroundImage = 'none';
    ambientEl.style.opacity = '0';
  }

  async function loadLyrics(trackId) {
    const container = document.getElementById('lyrics-container');
    if (!container) return;
    lastActiveLyricIdx = -1;
    isUserScrolling = false;
    const resumeBtn = document.getElementById('btn-lyrics-resume');
    if (resumeBtn) resumeBtn.classList.add('hidden');
    updateLyricsAmbientBackdrop();

    try {
      const res = await fetch(`${API_BASE}/api/lyrics/${trackId}`);
      if (!res.ok) return;
      const data = await res.json();
      if (!data || (!data.plain_lyrics && (!data.synced_lyrics || data.synced_lyrics.length === 0))) {
        container.innerHTML = '<div class="lyrics-placeholder">No lyrics available for this track</div>';
        currentLyrics = { plain: '', synced: [] };
        return;
      }
      currentLyrics = {
        plain: data.plain_lyrics || '',
        synced: data.synced_lyrics || [],
      };
      renderLyrics();
    } catch (e) {
      container.innerHTML = '<div class="lyrics-placeholder">No lyrics available</div>';
    }
  }

  function renderLyrics() {
    const container = document.getElementById('lyrics-container');
    if (!container) return;
    container.innerHTML = '';

    if (currentLyrics.synced.length > 0) {
      currentLyrics.synced.forEach((item, idx) => {
        const p = document.createElement('p');
        p.className = 'lyric-line';
        p.id = `lyric-line-${idx}`;
        p.textContent = item.text;
        p.addEventListener('click', () => {
          sendCommand({ Seek: item.time_secs });
          isUserScrolling = false;
          lastActiveLyricIdx = idx;
          const resumeBtn = document.getElementById('btn-lyrics-resume');
          if (resumeBtn) resumeBtn.classList.add('hidden');
          if (userScrollTimeout) clearTimeout(userScrollTimeout);
          scrollToLyric(p, true);
        });
        container.appendChild(p);
      });
    } else if (currentLyrics.plain) {
      const pre = document.createElement('pre');
      pre.style.whiteSpace = 'pre-wrap';
      pre.style.fontFamily = 'inherit';
      pre.textContent = currentLyrics.plain;
      container.appendChild(pre);
    }
  }

  function markUserScrolling(e) {
    if (!currentLyrics.synced || currentLyrics.synced.length === 0) return;

    // Ignore scroll events originating from programmatic scrollIntoView
    if (e && e.type === 'scroll' && isProgrammaticScroll && !isUserDraggingScrollbar) {
      return;
    }

    isProgrammaticScroll = false;
    if (programmaticScrollTimeout) clearTimeout(programmaticScrollTimeout);

    isUserScrolling = true;
    const resumeBtn = document.getElementById('btn-lyrics-resume');
    if (resumeBtn) resumeBtn.classList.remove('hidden');
    if (userScrollTimeout) clearTimeout(userScrollTimeout);
    userScrollTimeout = setTimeout(() => {
      isUserScrolling = false;
      if (resumeBtn) resumeBtn.classList.add('hidden');
    }, 5000);
  }

  const lyricsContainer = document.getElementById('lyrics-container');
  if (lyricsContainer) {
    lyricsContainer.addEventListener('wheel', markUserScrolling, { passive: true });
    lyricsContainer.addEventListener('touchmove', markUserScrolling, { passive: true });
    lyricsContainer.addEventListener('pointerdown', () => { isUserDraggingScrollbar = true; }, { passive: true });
    window.addEventListener('pointerup', () => { isUserDraggingScrollbar = false; }, { passive: true });
    window.addEventListener('pointercancel', () => { isUserDraggingScrollbar = false; }, { passive: true });
    lyricsContainer.addEventListener('scroll', markUserScrolling, { passive: true });
  }

  document.getElementById('btn-lyrics-resume')?.addEventListener('click', () => {
    isUserScrolling = false;
    const resumeBtn = document.getElementById('btn-lyrics-resume');
    if (resumeBtn) resumeBtn.classList.add('hidden');
    if (userScrollTimeout) clearTimeout(userScrollTimeout);
    if (lastActiveLyricIdx >= 0) {
      const activeEl = document.getElementById(`lyric-line-${lastActiveLyricIdx}`);
      if (activeEl) {
        scrollToLyric(activeEl, true);
      }
    }
  });

  function syncActiveLyric(curPos) {
    if (!currentLyrics.synced || currentLyrics.synced.length === 0) return;
    let activeIdx = -1;
    for (let i = 0; i < currentLyrics.synced.length; i++) {
      if (currentLyrics.synced[i].time_secs <= curPos) {
        activeIdx = i;
      } else {
        break;
      }
    }

    document.querySelectorAll('.lyric-line').forEach((l, idx) => {
      if (idx === activeIdx) {
        l.classList.add('active');
      } else {
        l.classList.remove('active');
      }
    });

    if (!isUserScrolling && activeIdx !== lastActiveLyricIdx && activeIdx >= 0) {
      lastActiveLyricIdx = activeIdx;
      const activeEl = document.getElementById(`lyric-line-${activeIdx}`);
      if (activeEl) {
        scrollToLyric(activeEl, true);
      }
    }
  }

  // 9. Themes & Settings
  document.querySelectorAll('.theme-card').forEach(card => {
    card.addEventListener('click', async () => {
      const theme = card.getAttribute('data-theme');
      document.documentElement.dataset.theme = theme;
      document.querySelectorAll('.theme-card').forEach(c => c.classList.remove('active'));
      card.classList.add('active');

      try {
        await fetch(`${API_BASE}/api/theme`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ theme }),
        });
      } catch (e) {}
    });
  });

  // Spotify Import from Browser
  document.getElementById('btn-spotify-import')?.addEventListener('click', async () => {
    const input = document.getElementById('spotify-input');
    const statusText = document.getElementById('spotify-status-text');
    statusText.textContent = 'Importing Spotify session from browser...';

    try {
      const res = await fetch(`${API_BASE}/api/spotify/import-browser`, { method: 'POST' });
      const data = await res.json();
      if (data.success) {
        if (data.sp_dc && input) input.value = data.sp_dc;
        statusText.textContent = `✓ ${data.message || 'Successfully imported from browser!'}`;
        setTimeout(() => loadPlaylists(), 1500);
      } else {
        statusText.textContent = `Import failed: ${data.message || 'No session found'}`;
      }
    } catch (e) {
      statusText.textContent = `Error: ${e.message}`;
    }
  });

  // Spotify Launch Login Window
  let loginPollInterval = null;
  document.getElementById('btn-spotify-login')?.addEventListener('click', async () => {
    const input = document.getElementById('spotify-input');
    const statusText = document.getElementById('spotify-status-text');
    const btn = document.getElementById('btn-spotify-login');
    statusText.textContent = 'Opening Spotify login window...';
    if (btn) btn.disabled = true;

    try {
      const res = await fetch(`${API_BASE}/api/spotify/launch-login`, { method: 'POST' });
      const data = await res.json();
      if (!data.success) {
        statusText.textContent = `Failed: ${data.message}`;
        if (btn) btn.disabled = false;
        return;
      }

      statusText.textContent = 'Waiting for login in Spotify window...';

      if (loginPollInterval) clearInterval(loginPollInterval);
      loginPollInterval = setInterval(async () => {
        try {
          const stRes = await fetch(`${API_BASE}/api/spotify/login-status`);
          const stData = await stRes.json();
          if (stData.status === 'success') {
            clearInterval(loginPollInterval);
            if (btn) btn.disabled = false;
            if (stData.sp_dc && input) input.value = stData.sp_dc;
            statusText.textContent = `✓ ${stData.message || 'Successfully logged in to Spotify!'}`;
            loadPlaylists();
          } else if (stData.status === 'error') {
            clearInterval(loginPollInterval);
            if (btn) btn.disabled = false;
            statusText.textContent = `Login failed: ${stData.message}`;
          }
        } catch (err) {
          // ignore transient poll error
        }
      }, 1500);
    } catch (e) {
      statusText.textContent = `Error: ${e.message}`;
      if (btn) btn.disabled = false;
    }
  });

  // Spotify Sync
  document.getElementById('btn-sync-spotify')?.addEventListener('click', async () => {
    const input = document.getElementById('spotify-input');
    const statusText = document.getElementById('spotify-status-text');
    const val = input ? input.value.trim() : '';

    statusText.textContent = 'Syncing Spotify Liked Songs and Playlists...';
    try {
      const res = await fetch(`${API_BASE}/api/spotify/sync`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ sp_dc: val || null }),
      });
      const data = await res.json();
      if (data.success) {
        const trCount = data.synced_tracks ?? data.tracks_imported ?? 0;
        const plCount = data.synced_playlists ?? data.playlists_imported ?? 0;
        statusText.textContent = `Sync complete! Imported ${trCount} songs and ${plCount} playlists.`;
        loadPlaylists();
      } else {
        statusText.textContent = `Sync failed: ${data.message || 'Unknown error'}`;
      }
    } catch (e) {
      statusText.textContent = `Error: ${e.message}`;
    }
  });

  async function checkSpotifyStatus() {
    try {
      const res = await fetch(`${API_BASE}/api/spotify/status`);
      if (res.ok) {
        const data = await res.json();
        const input = document.getElementById('spotify-input');
        const statusText = document.getElementById('spotify-status-text');
        if (data.configured && data.sp_dc) {
          if (input && !input.value) input.value = data.sp_dc;
          if (statusText && !statusText.textContent) statusText.textContent = '✓ Connected to Spotify';
        }
      }
    } catch (_) {}
  }

  // 10. Audio Hardware Output Device Selector
  const audioDeviceSelect = document.getElementById('audio-device-select');
  const btnRefreshAudioDevices = document.getElementById('btn-refresh-audio-devices');
  const audioDeviceStatus = document.getElementById('audio-device-status');

  async function loadAudioDevices() {
    if (!audioDeviceSelect) return;
    try {
      const res = await fetch(`${API_BASE}/api/audio/devices`);
      if (!res.ok) return;
      const devices = await res.json();
      audioDeviceSelect.innerHTML = '';
      if (devices.length === 0) {
        audioDeviceSelect.innerHTML = '<option value="">No audio sinks detected</option>';
        return;
      }
      devices.forEach(d => {
        const opt = document.createElement('option');
        opt.value = d.name;
        opt.textContent = `${d.description}${d.is_default ? ' (Active Default)' : ''}`;
        if (d.is_default) opt.selected = true;
        audioDeviceSelect.appendChild(opt);
      });
    } catch (e) {
      console.error('Failed to load audio devices:', e);
    }
  }

  audioDeviceSelect?.addEventListener('change', async (e) => {
    const selectedName = e.target.value;
    if (!selectedName) return;
    if (audioDeviceStatus) audioDeviceStatus.textContent = 'Switching output device...';
    try {
      const res = await fetch(`${API_BASE}/api/audio/devices/select`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name: selectedName }),
      });
      const ok = await res.json();
      if (ok) {
        if (audioDeviceStatus) audioDeviceStatus.textContent = '✓ Output device switched successfully';
        showToast('Audio output device switched');
        loadAudioDevices();
      } else {
        if (audioDeviceStatus) audioDeviceStatus.textContent = 'Failed to switch device';
      }
    } catch (err) {
      if (audioDeviceStatus) audioDeviceStatus.textContent = `Error: ${err.message}`;
    }
  });

  btnRefreshAudioDevices?.addEventListener('click', () => {
    loadAudioDevices();
    showToast('Refreshed audio devices');
  });

  // 11. Music Download Directory Config
  const settingDownloadPath = document.getElementById('setting-download-path');
  const btnSaveDownloadPath = document.getElementById('btn-save-download-path');
  const downloadPathStatus = document.getElementById('download-path-status');

  async function loadDownloadPath() {
    if (!settingDownloadPath) return;
    try {
      const res = await fetch(`${API_BASE}/api/settings/download_path`);
      if (res.ok) {
        const val = await res.json();
        if (val) settingDownloadPath.value = val;
      }
    } catch (_) {}
  }

  btnSaveDownloadPath?.addEventListener('click', async () => {
    const p = settingDownloadPath ? settingDownloadPath.value.trim() : '';
    if (!p) return;
    try {
      const res = await fetch(`${API_BASE}/api/settings`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ key: 'download_path', value: p }),
      });
      if (res.ok) {
        if (downloadPathStatus) downloadPathStatus.textContent = '✓ Download directory saved';
        showToast('Download path saved');
      }
    } catch (e) {
      if (downloadPathStatus) downloadPathStatus.textContent = `Error: ${e.message}`;
    }
  });

  // 12. Global Keyboard Shortcuts
  window.addEventListener('keydown', (e) => {
    const tag = document.activeElement?.tagName?.toLowerCase();
    if (tag === 'input' || tag === 'textarea' || tag === 'select') return;

    if (e.code === 'Space') {
      e.preventDefault();
      sendCommand('PlayPause');
    } else if (e.code === 'ArrowLeft') {
      e.preventDefault();
      sendCommand({ SeekRelative: -5.0 });
    } else if (e.code === 'ArrowRight') {
      e.preventDefault();
      sendCommand({ SeekRelative: 5.0 });
    } else if (e.code === 'ArrowUp') {
      e.preventDefault();
      const currentVol = currentState.volume !== undefined ? currentState.volume : 0.5;
      const newVol = Math.min(1.0, currentVol + 0.05);
      sendCommand({ SetVolume: newVol });
      if (volumeSlider) volumeSlider.value = newVol;
      if (volumeText) volumeText.textContent = `${Math.round(newVol * 100)}%`;
      updateMuteIcon(newVol);
    } else if (e.code === 'ArrowDown') {
      e.preventDefault();
      const currentVol = currentState.volume !== undefined ? currentState.volume : 0.5;
      const newVol = Math.max(0.0, currentVol - 0.05);
      sendCommand({ SetVolume: newVol });
      if (volumeSlider) volumeSlider.value = newVol;
      if (volumeText) volumeText.textContent = `${Math.round(newVol * 100)}%`;
      updateMuteIcon(newVol);
    } else if (e.key === 'm' || e.key === 'M') {
      e.preventDefault();
      toggleMute();
    } else if (e.key === 'n' || e.key === 'N') {
      e.preventDefault();
      sendCommand('Next');
    } else if (e.key === 'p' || e.key === 'P') {
      e.preventDefault();
      sendCommand('Previous');
    }
  });

  // Initial load & Polling
  loadPlaylists();
  loadEqConfig();
  loadAudioDevices();
  loadDownloadPath();
  checkSpotifyStatus();
  fetchState();
  setInterval(fetchState, 500);

})();
