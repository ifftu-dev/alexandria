// IRL Review — file upload + skill tag input + comment. On submit, asks
// the host to queue the work to the local instructor inbox; polls the
// host for review replies.
//
// Talks to the host through window.alex. Submissions use alex.submit()
// with metadata.type = 'irl_review' so PluginHost routes to the
// irl_submit_for_review IPC. Reply lookups use alex.emitEvent('irl_refresh')
// which the host answers via an onHost message with the latest rows.

(function () {
  'use strict';

  let config = {
    default_skills: [],
    prompt: '',
    accept: '*/*',
    max_file_bytes: 20 * 1024 * 1024,
  };

  const state = {
    files: [], // { name, mime, size, data_b64 }
    skills: [], // string[]
  };

  // ----- DOM refs -----
  const promptEl = document.getElementById('prompt');
  const dropEl = document.getElementById('drop');
  const filesInput = document.getElementById('files');
  const acceptHint = document.getElementById('accept-hint');
  const fileListEl = document.getElementById('file-list');
  const skillInput = document.getElementById('skill-input');
  const skillTagsEl = document.getElementById('skill-tags');
  const commentEl = document.getElementById('comment');
  const submitBtn = document.getElementById('submit-btn');
  const refreshBtn = document.getElementById('refresh');
  const statusEl = document.getElementById('status');
  const reviewsListEl = document.getElementById('reviews-list');
  const form = document.getElementById('submit-form');

  // ----- Rendering -----
  function applyConfig() {
    if (config.prompt) {
      promptEl.textContent = config.prompt;
      promptEl.hidden = false;
    } else {
      promptEl.hidden = true;
    }
    filesInput.accept = config.accept;
    acceptHint.textContent =
      config.accept && config.accept !== '*/*' ? `Accepted: ${config.accept}` : '';
    state.skills = [...config.default_skills];
    renderSkills();
  }

  function renderFiles() {
    fileListEl.innerHTML = '';
    state.files.forEach((f, i) => {
      const row = document.createElement('div');
      row.className = 'file-row';
      const label = document.createElement('div');
      label.textContent = f.name;
      const meta = document.createElement('span');
      meta.className = 'meta';
      meta.textContent = `${formatBytes(f.size)} · ${f.mime || 'unknown'}`;
      label.appendChild(meta);
      const remove = document.createElement('button');
      remove.type = 'button';
      remove.textContent = 'Remove';
      remove.addEventListener('click', () => {
        state.files.splice(i, 1);
        renderFiles();
      });
      row.appendChild(label);
      row.appendChild(remove);
      fileListEl.appendChild(row);
    });
  }

  function renderSkills() {
    skillTagsEl.innerHTML = '';
    state.skills.forEach((s, i) => {
      const tag = document.createElement('span');
      tag.className = 'skill-tag';
      tag.textContent = s;
      const x = document.createElement('button');
      x.type = 'button';
      x.textContent = '×';
      x.setAttribute('aria-label', `Remove ${s}`);
      x.addEventListener('click', () => {
        state.skills.splice(i, 1);
        renderSkills();
      });
      tag.appendChild(x);
      skillTagsEl.appendChild(tag);
    });
  }

  function formatBytes(n) {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${(n / 1024 / 1024).toFixed(1)} MB`;
  }

  function setStatus(msg, kind) {
    statusEl.textContent = msg || '';
    statusEl.style.color =
      kind === 'error' ? '#dc2626' : kind === 'ok' ? '#16a34a' : '';
  }

  // Trigger a browser download for a stored attachment ({ name, mime, data_b64 }).
  // Requires the iframe's `allow-downloads` sandbox token.
  function downloadFile(f) {
    try {
      const bin = atob(f.data_b64 || '');
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      const blob = new Blob([bytes], { type: f.mime || 'application/octet-stream' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = f.name || 'download';
      document.body.appendChild(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(url), 1500);
    } catch (err) {
      setStatus(`Could not download "${f.name}": ${err && err.message ? err.message : err}`, 'error');
    }
  }

  // Load a past submission back into the editable form (view / resubmit).
  function loadSubmissionIntoForm(payload, skills) {
    commentEl.value = payload && payload.comment ? payload.comment : '';
    state.files = payload && Array.isArray(payload.files) ? payload.files.map((f) => ({ ...f })) : [];
    state.skills = Array.isArray(skills) ? [...skills] : [];
    renderFiles();
    renderSkills();
    setStatus('Loaded a past submission into the form above.', 'ok');
    if (form && form.scrollIntoView) form.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  function renderReviews(submissions) {
    if (!submissions || submissions.length === 0) {
      reviewsListEl.innerHTML = '<p class="sub">No submissions yet.</p>';
      return;
    }
    reviewsListEl.innerHTML = '';
    submissions.forEach((s) => {
      const card = document.createElement('div');
      card.className = 'review-card';

      const row = document.createElement('div');
      row.className = 'row';
      const left = document.createElement('div');
      const created = new Date(s.created_at);
      left.textContent = `Submitted ${created.toLocaleString()}`;
      const badge = document.createElement('span');
      badge.className = `badge ${s.status}`;
      badge.textContent = s.status;
      row.appendChild(left);
      row.appendChild(badge);
      card.appendChild(row);

      let submissionPayload = {};
      try { submissionPayload = JSON.parse(s.submission_json || '{}'); } catch (_) {}
      if (submissionPayload.comment) {
        const c = document.createElement('p');
        c.style.margin = '4px 0';
        c.style.fontSize = '13px';
        c.textContent = `“${submissionPayload.comment}”`;
        card.appendChild(c);
      }

      let skills = [];
      try { skills = JSON.parse(s.skills_json || '[]'); } catch (_) {}
      if (skills.length > 0) {
        const sk = document.createElement('div');
        sk.className = 'sub';
        sk.style.marginTop = '4px';
        sk.textContent = `Skills declared: ${skills.join(', ')}`;
        card.appendChild(sk);
      }

      if (s.status === 'reviewed') {
        const sc = document.createElement('div');
        sc.style.marginTop = '8px';
        sc.style.fontWeight = '600';
        sc.textContent = `Score: ${Math.round((s.score || 0) * 100)}%`;
        card.appendChild(sc);

        if (s.feedback) {
          const fb = document.createElement('p');
          fb.style.margin = '6px 0';
          fb.style.fontSize = '13px';
          fb.textContent = s.feedback;
          card.appendChild(fb);
        }

        if (s.skill_ratings_json) {
          let ratings = {};
          try { ratings = JSON.parse(s.skill_ratings_json); } catch (_) {}
          const keys = Object.keys(ratings);
          if (keys.length > 0) {
            const grid = document.createElement('div');
            grid.className = 'skill-ratings';
            keys.forEach((k) => {
              const name = document.createElement('div');
              name.textContent = k;
              const val = document.createElement('div');
              val.textContent = `${Math.round((Number(ratings[k]) || 0) * 100)}%`;
              grid.appendChild(name);
              grid.appendChild(val);
            });
            card.appendChild(grid);
          }
        }
      }

      // Attachments — downloadable from the snapshot.
      const attachedFiles = Array.isArray(submissionPayload.files) ? submissionPayload.files : [];
      if (attachedFiles.length > 0) {
        const filesWrap = document.createElement('div');
        filesWrap.style.marginTop = '8px';
        const heading = document.createElement('div');
        heading.className = 'sub';
        heading.textContent = 'Attachments';
        filesWrap.appendChild(heading);
        attachedFiles.forEach((f) => {
          const frow = document.createElement('div');
          frow.className = 'file-row';
          const nm = document.createElement('div');
          nm.textContent = f.name;
          const meta = document.createElement('span');
          meta.className = 'meta';
          meta.textContent = ` ${formatBytes(f.size || 0)} · ${f.mime || 'unknown'}`;
          nm.appendChild(meta);
          const dl = document.createElement('button');
          dl.type = 'button';
          dl.textContent = 'Download';
          dl.addEventListener('click', (e) => {
            e.stopPropagation();
            downloadFile(f);
          });
          frow.appendChild(nm);
          frow.appendChild(dl);
          filesWrap.appendChild(frow);
        });
        card.appendChild(filesWrap);
      }

      // Click the card (outside the download buttons) to load this submission
      // back into the editable form above.
      card.style.cursor = 'pointer';
      card.title = 'Click to load this submission into the form';
      card.addEventListener('click', () => loadSubmissionIntoForm(submissionPayload, skills));

      reviewsListEl.appendChild(card);
    });
  }

  // ----- File handling -----
  async function addFile(file) {
    if (file.size > config.max_file_bytes) {
      setStatus(`File "${file.name}" exceeds ${formatBytes(config.max_file_bytes)} cap.`, 'error');
      return;
    }
    const b64 = await fileToBase64(file);
    state.files.push({
      name: file.name,
      mime: file.type || 'application/octet-stream',
      size: file.size,
      data_b64: b64,
    });
    renderFiles();
  }

  function fileToBase64(file) {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onerror = () => reject(reader.error);
      reader.onload = () => {
        const result = String(reader.result || '');
        const comma = result.indexOf(',');
        resolve(comma >= 0 ? result.slice(comma + 1) : result);
      };
      reader.readAsDataURL(file);
    });
  }

  function uint8ToBase64(u8) {
    let s = '';
    const chunk = 0x8000;
    for (let i = 0; i < u8.length; i += chunk) {
      s += String.fromCharCode.apply(null, u8.subarray(i, i + chunk));
    }
    return btoa(s);
  }

  const MIME_BY_EXT = {
    png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif',
    webp: 'image/webp', pdf: 'application/pdf', mp4: 'video/mp4', mov: 'video/quicktime',
    mp3: 'audio/mpeg', wav: 'audio/wav', txt: 'text/plain',
  };

  // Files chosen through the host's native picker (the sandboxed iframe can't
  // show its own). Each: { name, size, data: Uint8Array }.
  async function addPickedFile(f) {
    if (f.size > config.max_file_bytes) {
      setStatus(`File "${f.name}" exceeds ${formatBytes(config.max_file_bytes)} cap.`, 'error');
      return;
    }
    const ext = (f.name.split('.').pop() || '').toLowerCase();
    state.files.push({
      name: f.name,
      mime: MIME_BY_EXT[ext] || 'application/octet-stream',
      size: f.size,
      data_b64: uint8ToBase64(f.data instanceof Uint8Array ? f.data : new Uint8Array(f.data)),
    });
    renderFiles();
  }

  filesInput.addEventListener('change', async () => {
    for (const f of Array.from(filesInput.files || [])) await addFile(f);
    filesInput.value = '';
  });

  // The sandboxed iframe can't open a native file dialog from <input type=file>,
  // so route clicks through the host's picker via the alex bridge.
  dropEl.addEventListener('click', async (e) => {
    e.preventDefault();
    try {
      const res = await alex.pickFiles({ multiple: true });
      if (res && Array.isArray(res.files)) {
        for (const f of res.files) await addPickedFile(f);
      }
    } catch (err) {
      setStatus(`Could not open file picker: ${err && err.message ? err.message : err}`, 'error');
    }
  });

  dropEl.addEventListener('dragover', (e) => {
    e.preventDefault();
    dropEl.classList.add('drag-over');
  });
  dropEl.addEventListener('dragleave', () => dropEl.classList.remove('drag-over'));
  dropEl.addEventListener('drop', async (e) => {
    e.preventDefault();
    dropEl.classList.remove('drag-over');
    for (const f of Array.from(e.dataTransfer && e.dataTransfer.files ? e.dataTransfer.files : [])) {
      await addFile(f);
    }
  });

  // ----- Skill tag input -----
  skillInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' || e.key === ',') {
      e.preventDefault();
      const val = skillInput.value.trim().replace(/,$/, '');
      if (val && !state.skills.includes(val)) {
        state.skills.push(val);
        renderSkills();
      }
      skillInput.value = '';
    }
  });

  // ----- Submit -----
  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    if (state.files.length === 0) {
      setStatus('Attach at least one file before submitting.', 'error');
      return;
    }
    submitBtn.disabled = true;
    setStatus('Submitting…');

    try {
      const submission = {
        files: state.files,
        comment: commentEl.value.trim(),
      };
      const resp = await alex.submit(submission, {
        type: 'irl_review',
        skills: state.skills,
      });
      const id = (resp && resp.submission_id) || '';
      setStatus(
        id
          ? `Submitted (id ${id.slice(0, 8)}). An instructor will review.`
          : 'Submitted. An instructor will review.',
        'ok',
      );
      state.files = [];
      commentEl.value = '';
      renderFiles();
      void alex.complete(1, null);
      await refreshSubmissions();
    } catch (err) {
      setStatus(`Submit failed: ${err.message || err}`, 'error');
    } finally {
      submitBtn.disabled = false;
    }
  });

  // ----- Refresh: ask host for my submissions -----
  let lastRefreshAt = 0;
  async function refreshSubmissions(showFeedback) {
    lastRefreshAt = Date.now();
    try {
      const resp = await alex.emitEvent('irl_refresh', {});
      const submissions = (resp && resp.submissions) || [];
      renderReviews(submissions);
      if (showFeedback) {
        const reviewed = submissions.filter((s) => s.status === 'reviewed').length;
        const n = submissions.length;
        setStatus(
          n === 0
            ? 'No submissions yet.'
            : `${n} submission${n === 1 ? '' : 's'}${reviewed ? `, ${reviewed} reviewed` : ' — awaiting review'}.`,
          'ok',
        );
      }
    } catch (err) {
      setStatus(`Could not load submissions: ${err.message || err}`, 'error');
    }
  }
  // Manual refresh checks for new instructor review replies (status/score/
  // feedback). Show a status line so the button visibly does something even
  // when nothing changed.
  refreshBtn.addEventListener('click', () => {
    setStatus('Checking for updates…');
    void refreshSubmissions(true);
  });

  // Poll periodically for review replies — host returns instantly from
  // the in-memory SQLite read so the cost is negligible.
  setInterval(() => {
    if (Date.now() - lastRefreshAt > 25000) void refreshSubmissions();
  }, 30000);

  // ----- Host wiring -----
  alex.onHost((msg) => {
    if (msg.type === 'init') {
      const content = (msg.payload && msg.payload.content) || {};
      if (Array.isArray(content.default_skills)) config.default_skills = content.default_skills;
      if (typeof content.prompt === 'string') config.prompt = content.prompt;
      if (typeof content.accept === 'string') config.accept = content.accept;
      if (typeof content.max_file_bytes === 'number') config.max_file_bytes = content.max_file_bytes;
      applyConfig();
      void refreshSubmissions();
    }
  });

  applyConfig();
  void alex.ready([]);
})();
