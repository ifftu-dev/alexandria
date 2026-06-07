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

  filesInput.addEventListener('change', async () => {
    for (const f of Array.from(filesInput.files || [])) await addFile(f);
    filesInput.value = '';
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
  async function refreshSubmissions() {
    lastRefreshAt = Date.now();
    try {
      const resp = await alex.emitEvent('irl_refresh', {});
      const submissions = (resp && resp.submissions) || [];
      renderReviews(submissions);
    } catch (err) {
      setStatus(`Could not load submissions: ${err.message || err}`, 'error');
    }
  }
  refreshBtn.addEventListener('click', () => void refreshSubmissions());

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
