//! Seed data for a fresh Alexandria database.
//!
//! Inserts a representative taxonomy (subject fields → subjects → skills),
//! prerequisite/relation edges, a governance DAO, and a sample course
//! with chapters, elements, and skill tags — giving new users something
//! to explore immediately.
//!
//! The seed function is idempotent: it only runs when the `subject_fields`
//! table is empty.

use rusqlite::Connection;

/// Seed the database with demo taxonomy, courses, and governance data.
/// Returns `Ok(true)` if seed data was inserted, `Ok(false)` if skipped.
pub fn seed_if_empty(conn: &Connection) -> Result<bool, rusqlite::Error> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM subject_fields", [], |row| row.get(0))?;

    if count > 0 {
        log::info!("Database already has taxonomy data — skipping seed");
        // Still backfill new demo data for existing databases
        backfill_demo_data(conn)?;
        return Ok(false);
    }

    log::info!("Seeding database with demo taxonomy, courses, and governance data…");

    conn.execute_batch(SEED_SQL)?;
    conn.execute_batch(BACKFILL_SQL)?;

    // Visual assets are applied via parameterized queries (not execute_batch)
    // because sqlite3_exec can silently fail on long SVG strings or emoji.
    seed_visual_assets(conn)?;

    // Inline content for all seed elements — stored directly in the database
    // so content is available on all platforms (including mobile without iroh).
    seed_inline_content(conn)?;

    log::info!("Seed data inserted successfully");
    Ok(true)
}

/// Backfill demo data for tables added after the initial seed.
/// Checks each table independently so it's safe to run on any existing DB.
fn backfill_demo_data(conn: &Connection) -> Result<(), rusqlite::Error> {
    let needs_backfill = |table: &str| -> bool {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0)
            == 0
    };

    // Only backfill if the core new tables are empty
    if needs_backfill("enrollments")
        || needs_backfill("governance_dao_members")
        || needs_backfill("classrooms")
    {
        log::info!("Backfilling demo data for new tables…");
        conn.execute_batch(BACKFILL_SQL)?;
        log::info!("Demo data backfill complete");
    }

    Ok(())
}

/// Store element content directly in the `content_inline` column.
/// This makes content available on all platforms (including mobile without iroh).
fn seed_inline_content(conn: &Connection) -> Result<(), rusqlite::Error> {
    use super::seed_content::SEED_CONTENT;
    use rusqlite::params;

    for (element_id, body) in SEED_CONTENT {
        conn.execute(
            "UPDATE course_elements SET content_inline = ?1 WHERE id = ?2",
            params![body, element_id],
        )?;
    }
    Ok(())
}

/// Apply visual assets (emojis, author names, thumbnails) via parameterized queries.
fn seed_visual_assets(conn: &Connection) -> Result<(), rusqlite::Error> {
    use rusqlite::params;

    // Subject field emojis
    for (id, emoji) in [
        ("sf_cs", "\u{1F4BB}"),     // 💻
        ("sf_math", "\u{1F4D0}"),   // 📐
        ("sf_data", "\u{1F4CA}"),   // 📊
        ("sf_web", "\u{1F310}"),    // 🌐
        ("sf_cyber", "\u{1F510}"),  // 🔐
        ("sf_design", "\u{1F3A8}"), // 🎨
    ] {
        conn.execute(
            "UPDATE subject_fields SET icon_emoji = ?1 WHERE id = ?2",
            params![emoji, id],
        )?;
    }

    // DAO emojis
    for (id, emoji) in [
        ("dao_cs", "\u{1F4BB}"),
        ("dao_math", "\u{1F4D0}"),
        ("dao_data", "\u{1F4CA}"),
        ("dao_web", "\u{1F310}"),
        ("dao_cyber", "\u{1F510}"),
        ("dao_design", "\u{1F3A8}"),
    ] {
        conn.execute(
            "UPDATE governance_daos SET icon_emoji = ?1 WHERE id = ?2",
            params![emoji, id],
        )?;
    }

    // Course author display names
    for (id, name) in [
        ("course_algo_101", "Dr. Elena Vasquez"),
        ("course_web_fullstack", "Dr. Elena Vasquez"),
        ("course_ml_foundations", "Marcus Chen"),
        ("course_crypto_101", "Marcus Chen"),
        ("course_ux_design", "Amara Osei"),
        ("course_math_discrete", "Prof. Imani Okafor"),
    ] {
        conn.execute(
            "UPDATE courses SET author_name = ?1 WHERE id = ?2",
            params![name, id],
        )?;
    }

    // Course thumbnail SVGs
    for (id, svg) in COURSE_THUMBNAILS {
        conn.execute(
            "UPDATE courses SET thumbnail_svg = ?1 WHERE id = ?2",
            params![svg, id],
        )?;
    }

    Ok(())
}

const COURSE_THUMBNAILS: &[(&str, &str)] = &[
    (
        "course_algo_101",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="#6366f1"/><stop offset="100%" stop-color="#8b5cf6"/></linearGradient></defs><rect width="640" height="360" fill="url(#g)"/><g opacity="0.15" fill="none" stroke="#fff" stroke-width="2"><line x1="80" y1="280" x2="160" y2="200"/><line x1="160" y1="200" x2="240" y2="120"/><line x1="240" y1="120" x2="320" y2="180"/><line x1="320" y1="180" x2="400" y2="80"/><line x1="400" y1="80" x2="480" y2="160"/><line x1="480" y1="160" x2="560" y2="100"/><circle cx="80" cy="280" r="6" fill="#fff"/><circle cx="160" cy="200" r="6" fill="#fff"/><circle cx="240" cy="120" r="6" fill="#fff"/><circle cx="320" cy="180" r="6" fill="#fff"/><circle cx="400" cy="80" r="6" fill="#fff"/><circle cx="480" cy="160" r="6" fill="#fff"/><circle cx="560" cy="100" r="6" fill="#fff"/></g><g opacity="0.08" fill="#fff"><rect x="100" y="240" width="40" height="80" rx="4"/><rect x="160" y="200" width="40" height="120" rx="4"/><rect x="220" y="160" width="40" height="160" rx="4"/><rect x="280" y="180" width="40" height="140" rx="4"/><rect x="340" y="120" width="40" height="200" rx="4"/><rect x="400" y="100" width="40" height="220" rx="4"/><rect x="460" y="140" width="40" height="180" rx="4"/></g><text x="320" y="175" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="28" font-weight="700" opacity="0.9">O(n log n)</text><text x="320" y="210" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="14" opacity="0.6">Algorithms &amp; Data Structures</text></svg>"##,
    ),
    (
        "course_web_fullstack",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="#0ea5e9"/><stop offset="100%" stop-color="#6366f1"/></linearGradient></defs><rect width="640" height="360" fill="url(#g)"/><g opacity="0.1" fill="none" stroke="#fff" stroke-width="1.5"><rect x="60" y="40" width="200" height="280" rx="12"/><rect x="80" y="60" width="160" height="20" rx="4"/><rect x="80" y="90" width="120" height="12" rx="3"/><rect x="80" y="110" width="140" height="12" rx="3"/><rect x="80" y="130" width="100" height="12" rx="3"/><rect x="80" y="160" width="160" height="100" rx="6"/><rect x="80" y="270" width="70" height="28" rx="6"/><rect x="160" y="270" width="70" height="28" rx="6"/></g><g opacity="0.12" fill="#fff"><circle cx="440" cy="180" r="80"/><circle cx="440" cy="180" r="60" fill="none" stroke="#fff" stroke-width="2"/><path d="M420 160 L430 180 L460 180 L435 195 L445 215 L420 200 L395 215 L405 195 L380 180 L410 180Z"/></g><g opacity="0.07" fill="none" stroke="#fff" stroke-width="1"><line x1="340" y1="100" x2="540" y2="100"/><line x1="340" y1="130" x2="540" y2="130"/><line x1="340" y1="230" x2="540" y2="230"/><line x1="340" y1="260" x2="540" y2="260"/></g><text x="320" y="170" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="24" font-weight="700" opacity="0.9">&lt;Vue /&gt; + Rust</text><text x="320" y="205" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="14" opacity="0.6">Full-Stack Web Development</text></svg>"##,
    ),
    (
        "course_ml_foundations",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="#f59e0b"/><stop offset="100%" stop-color="#ef4444"/></linearGradient></defs><rect width="640" height="360" fill="url(#g)"/><g opacity="0.12" fill="none" stroke="#fff" stroke-width="2"><circle cx="200" cy="180" r="8"/><circle cx="280" cy="120" r="8"/><circle cx="360" cy="200" r="8"/><circle cx="440" cy="140" r="8"/><circle cx="320" cy="80" r="8"/><circle cx="160" cy="260" r="8"/><circle cx="480" cy="240" r="8"/><circle cx="400" cy="280" r="8"/><circle cx="240" cy="220" r="8"/><circle cx="520" cy="160" r="8"/><line x1="200" y1="180" x2="280" y2="120"/><line x1="280" y1="120" x2="360" y2="200"/><line x1="360" y1="200" x2="440" y2="140"/><line x1="280" y1="120" x2="320" y2="80"/><line x1="200" y1="180" x2="160" y2="260"/><line x1="440" y1="140" x2="480" y2="240"/><line x1="360" y1="200" x2="400" y2="280"/><line x1="200" y1="180" x2="240" y2="220"/><line x1="440" y1="140" x2="520" y2="160"/></g><g opacity="0.08" fill="#fff"><circle cx="320" cy="180" r="100"/><ellipse cx="320" cy="180" rx="140" ry="60" fill="none" stroke="#fff" stroke-width="1" stroke-dasharray="4,4"/></g><text x="320" y="170" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="26" font-weight="700" opacity="0.9">f(x) = wx + b</text><text x="320" y="205" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="14" opacity="0.6">Machine Learning Foundations</text></svg>"##,
    ),
    (
        "course_crypto_101",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="#10b981"/><stop offset="100%" stop-color="#0ea5e9"/></linearGradient></defs><rect width="640" height="360" fill="url(#g)"/><g opacity="0.1" fill="none" stroke="#fff" stroke-width="2"><rect x="180" y="80" width="280" height="200" rx="16"/><circle cx="320" cy="140" r="30"/><path d="M290 140 L320 110 L350 140" stroke-width="3"/><line x1="240" y1="200" x2="400" y2="200"/><rect x="240" y="220" width="60" height="8" rx="4"/><rect x="320" y="220" width="80" height="8" rx="4"/></g><g opacity="0.07" fill="#fff"><circle cx="120" cy="100" r="4"/><circle cx="520" cy="80" r="4"/><circle cx="100" cy="260" r="4"/><circle cx="540" cy="280" r="4"/><line x1="120" y1="100" x2="180" y2="80" stroke="#fff" stroke-width="1"/><line x1="460" y1="80" x2="520" y2="80" stroke="#fff" stroke-width="1"/><line x1="100" y1="260" x2="180" y2="280" stroke="#fff" stroke-width="1"/><line x1="460" y1="280" x2="540" y2="280" stroke="#fff" stroke-width="1"/></g><g opacity="0.06"><rect x="60" y="300" width="520" height="30" rx="4" fill="#fff"/><rect x="70" y="306" width="100" height="18" rx="3" fill="none" stroke="#fff" stroke-width="1"/><rect x="180" y="306" width="80" height="18" rx="3" fill="none" stroke="#fff" stroke-width="1"/></g><text x="320" y="170" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="24" font-weight="700" opacity="0.9">AES-256 + Ed25519</text><text x="320" y="205" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="14" opacity="0.6">Applied Cryptography</text></svg>"##,
    ),
    (
        "course_ux_design",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="#ec4899"/><stop offset="100%" stop-color="#f59e0b"/></linearGradient></defs><rect width="640" height="360" fill="url(#g)"/><g opacity="0.1" fill="none" stroke="#fff" stroke-width="1.5"><rect x="100" y="60" width="180" height="240" rx="12"/><circle cx="190" cy="120" r="24"/><rect x="130" y="160" width="120" height="8" rx="4"/><rect x="140" y="178" width="100" height="6" rx="3"/><rect x="130" y="200" width="120" height="40" rx="6"/><rect x="130" y="250" width="50" height="28" rx="14"/><rect x="200" y="250" width="50" height="28" rx="14"/></g><g opacity="0.1" fill="none" stroke="#fff" stroke-width="1.5"><rect x="360" y="60" width="180" height="240" rx="12"/><rect x="390" y="90" width="120" height="80" rx="8"/><rect x="390" y="185" width="80" height="8" rx="4"/><rect x="390" y="205" width="120" height="6" rx="3"/><rect x="390" y="225" width="100" height="6" rx="3"/><rect x="390" y="255" width="60" height="24" rx="12"/></g><g opacity="0.07" fill="#fff"><path d="M300 160 L320 140 L340 160" stroke="#fff" stroke-width="2" fill="none"/><line x1="320" y1="160" x2="320" y2="200" stroke="#fff" stroke-width="2"/><circle cx="320" cy="220" r="4"/></g><text x="320" y="170" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="26" font-weight="700" opacity="0.9">UX Design</text><text x="320" y="205" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="14" opacity="0.6">Research to Prototype</text></svg>"##,
    ),
    (
        "course_math_discrete",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="#14b8a6"/><stop offset="100%" stop-color="#0ea5e9"/></linearGradient></defs><rect width="640" height="360" fill="url(#g)"/><g opacity="0.12" fill="none" stroke="#fff" stroke-width="2"><circle cx="160" cy="90" r="6"/><circle cx="250" cy="140" r="6"/><circle cx="350" cy="100" r="6"/><circle cx="450" cy="170" r="6"/><circle cx="540" cy="120" r="6"/><line x1="160" y1="90" x2="250" y2="140"/><line x1="250" y1="140" x2="350" y2="100"/><line x1="350" y1="100" x2="450" y2="170"/><line x1="450" y1="170" x2="540" y2="120"/></g><g opacity="0.08" fill="#fff"><rect x="90" y="220" width="460" height="90" rx="12"/><rect x="120" y="245" width="120" height="16" rx="4"/><rect x="260" y="245" width="120" height="16" rx="4"/><rect x="400" y="245" width="120" height="16" rx="4"/></g><text x="320" y="165" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="26" font-weight="700" opacity="0.92">Discrete Math</text><text x="320" y="200" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="14" opacity="0.65">Logic, Sets, Graphs, Probability</text></svg>"##,
    ),
];

const SEED_SQL: &str = r##"
-- ============================================================
-- SUBJECT FIELDS (top-level knowledge domains)
-- ============================================================
INSERT INTO subject_fields (id, name, description) VALUES
    ('sf_cs',       'Computer Science',     'The study of computation, algorithms, data structures, and software systems'),
    ('sf_math',     'Mathematics',          'Pure and applied mathematics including algebra, calculus, and discrete math'),
    ('sf_data',     'Data Science',         'Statistics, machine learning, data engineering, and analytical methods'),
    ('sf_web',      'Web Development',      'Frontend, backend, and full-stack web application development'),
    ('sf_cyber',    'Cybersecurity',        'Information security, cryptography, and defensive/offensive techniques'),
    ('sf_design',   'Design',              'User experience, interface design, and visual communication');

-- ============================================================
-- SUBJECTS (mid-level topics within fields)
-- ============================================================
INSERT INTO subjects (id, name, description, subject_field_id) VALUES
    -- Computer Science
    ('sub_algo',        'Algorithms & Data Structures',     'Fundamental algorithms, complexity analysis, and core data structures',       'sf_cs'),
    ('sub_os',          'Operating Systems',                'Process management, memory, file systems, and concurrency',                  'sf_cs'),
    ('sub_lang',        'Programming Languages',            'Language design, type systems, compilers, and paradigms',                    'sf_cs'),
    ('sub_net',         'Computer Networks',                'Protocols, routing, transport layers, and network architecture',             'sf_cs'),
    -- Mathematics
    ('sub_calc',        'Calculus',                         'Limits, derivatives, integrals, and multivariable calculus',                 'sf_math'),
    ('sub_linalg',      'Linear Algebra',                   'Vectors, matrices, transformations, and eigenvalues',                       'sf_math'),
    ('sub_discrete',    'Discrete Mathematics',             'Logic, sets, combinatorics, graph theory, and proofs',                      'sf_math'),
    ('sub_prob',        'Probability & Statistics',         'Random variables, distributions, inference, and hypothesis testing',         'sf_math'),
    -- Data Science
    ('sub_ml',          'Machine Learning',                 'Supervised, unsupervised, and reinforcement learning algorithms',            'sf_data'),
    ('sub_nlp',         'Natural Language Processing',      'Text processing, language models, and sequence-to-sequence architectures',   'sf_data'),
    ('sub_dataeng',     'Data Engineering',                 'Pipelines, ETL, warehousing, and streaming architectures',                  'sf_data'),
    -- Web Development
    ('sub_frontend',    'Frontend Development',             'HTML, CSS, JavaScript, frameworks, and browser APIs',                       'sf_web'),
    ('sub_backend',     'Backend Development',              'Server-side logic, APIs, databases, and authentication',                    'sf_web'),
    ('sub_devops',      'DevOps & Deployment',              'CI/CD, containers, infrastructure as code, and monitoring',                 'sf_web'),
    -- Cybersecurity
    ('sub_crypto',      'Cryptography',                     'Symmetric/asymmetric encryption, hashing, digital signatures',              'sf_cyber'),
    ('sub_netsec',      'Network Security',                 'Firewalls, intrusion detection, TLS, and secure protocols',                 'sf_cyber'),
    -- Design
    ('sub_ux',          'UX Design',                        'User research, information architecture, and interaction design',           'sf_design'),
    ('sub_ui',          'UI Design',                        'Visual design, typography, color theory, and design systems',               'sf_design');

-- ============================================================
-- SKILLS (leaf-level assessable abilities)
-- ============================================================
INSERT INTO skills (id, name, description, subject_id, bloom_level) VALUES
    -- Algorithms & Data Structures
    ('skill_big_o',         'Big-O Analysis',                   'Analyze time and space complexity of algorithms',                    'sub_algo',     'analyze'),
    ('skill_arrays',        'Arrays & Strings',                 'Implement and manipulate array-based data structures',               'sub_algo',     'apply'),
    ('skill_linked_lists',  'Linked Lists',                     'Implement singly and doubly linked list operations',                 'sub_algo',     'apply'),
    ('skill_stacks_queues', 'Stacks & Queues',                  'Implement LIFO and FIFO data structures and their applications',    'sub_algo',     'apply'),
    ('skill_trees',         'Trees & BSTs',                     'Implement binary trees, BSTs, and tree traversal algorithms',        'sub_algo',     'apply'),
    ('skill_graphs',        'Graph Algorithms',                 'Implement BFS, DFS, shortest paths, and spanning trees',            'sub_algo',     'apply'),
    ('skill_sorting',       'Sorting Algorithms',               'Implement and compare comparison and non-comparison sorts',          'sub_algo',     'apply'),
    ('skill_hashing',       'Hash Tables',                      'Implement hash maps with collision resolution strategies',           'sub_algo',     'apply'),
    ('skill_dp',            'Dynamic Programming',              'Solve optimization problems using memoization and tabulation',       'sub_algo',     'analyze'),
    ('skill_greedy',        'Greedy Algorithms',                'Design greedy strategies and prove correctness',                     'sub_algo',     'analyze'),

    -- Operating Systems
    ('skill_processes',     'Processes & Threads',              'Manage process lifecycle, threading models, and scheduling',         'sub_os',       'understand'),
    ('skill_memory',        'Memory Management',                'Explain paging, virtual memory, and memory allocation',              'sub_os',       'understand'),
    ('skill_concurrency',   'Concurrency & Synchronization',    'Use locks, semaphores, and message passing to prevent data races',  'sub_os',       'apply'),
    ('skill_filesystems',   'File Systems',                     'Describe file system structures, journaling, and I/O scheduling',   'sub_os',       'understand'),

    -- Programming Languages
    ('skill_rust',          'Rust Programming',                 'Write safe, concurrent Rust code using ownership and lifetimes',     'sub_lang',     'apply'),
    ('skill_typescript',    'TypeScript',                       'Build type-safe applications with TypeScript and its type system',   'sub_lang',     'apply'),
    ('skill_python',        'Python',                           'Write idiomatic Python for scripting, data, and web development',   'sub_lang',     'apply'),
    ('skill_functional',    'Functional Programming',           'Apply FP concepts: immutability, higher-order functions, monads',   'sub_lang',     'understand'),

    -- Computer Networks
    ('skill_tcp_ip',        'TCP/IP Stack',                     'Explain the TCP/IP model layers and protocol interactions',          'sub_net',      'understand'),
    ('skill_http',          'HTTP & REST',                      'Design and consume RESTful APIs using HTTP methods and status codes','sub_net',      'apply'),
    ('skill_dns',           'DNS',                              'Explain DNS resolution, record types, and caching',                 'sub_net',      'remember'),

    -- Calculus
    ('skill_limits',        'Limits & Continuity',              'Evaluate limits and determine continuity of functions',              'sub_calc',     'apply'),
    ('skill_derivatives',   'Derivatives',                      'Compute derivatives and apply differentiation rules',               'sub_calc',     'apply'),
    ('skill_integrals',     'Integration',                      'Evaluate definite and indefinite integrals using standard techniques','sub_calc',    'apply'),
    ('skill_multivariable', 'Multivariable Calculus',           'Compute partial derivatives, gradients, and multiple integrals',    'sub_calc',     'apply'),

    -- Linear Algebra
    ('skill_vectors',       'Vectors & Spaces',                 'Perform vector operations and reason about vector spaces',          'sub_linalg',   'apply'),
    ('skill_matrices',      'Matrix Operations',                'Multiply, invert, and decompose matrices',                          'sub_linalg',   'apply'),
    ('skill_eigenvalues',   'Eigenvalues & Eigenvectors',       'Compute eigenvalues/eigenvectors and apply spectral decomposition', 'sub_linalg',   'analyze'),
    ('skill_svd',           'Singular Value Decomposition',     'Compute and interpret SVD for dimensionality reduction',            'sub_linalg',   'analyze'),

    -- Discrete Mathematics
    ('skill_logic',         'Propositional & Predicate Logic',  'Construct and evaluate logical propositions and proofs',            'sub_discrete', 'apply'),
    ('skill_sets',          'Set Theory',                       'Apply set operations, relations, and functions',                     'sub_discrete', 'apply'),
    ('skill_combinatorics', 'Combinatorics',                    'Solve counting problems using permutations and combinations',       'sub_discrete', 'apply'),
    ('skill_graph_theory',  'Graph Theory',                     'Prove properties of graphs, trees, and network flows',              'sub_discrete', 'analyze'),

    -- Probability & Statistics
    ('skill_probability',   'Probability Fundamentals',         'Compute probabilities using axioms, Bayes theorem, and distributions','sub_prob',   'apply'),
    ('skill_distributions', 'Probability Distributions',        'Work with common distributions (normal, binomial, Poisson, etc.)', 'sub_prob',     'apply'),
    ('skill_inference',     'Statistical Inference',            'Perform hypothesis testing, confidence intervals, and estimation',  'sub_prob',     'analyze'),
    ('skill_regression',    'Regression Analysis',              'Build and interpret linear and logistic regression models',          'sub_prob',     'apply'),

    -- Machine Learning
    ('skill_supervised',    'Supervised Learning',              'Train and evaluate classification and regression models',            'sub_ml',       'apply'),
    ('skill_unsupervised',  'Unsupervised Learning',            'Apply clustering, dimensionality reduction, and anomaly detection', 'sub_ml',       'apply'),
    ('skill_neural_nets',   'Neural Networks',                  'Build and train feedforward and convolutional neural networks',      'sub_ml',       'apply'),
    ('skill_deep_learning', 'Deep Learning',                    'Design deep architectures: RNNs, transformers, GANs',               'sub_ml',       'create'),
    ('skill_ml_eval',       'Model Evaluation',                 'Apply cross-validation, metrics, and bias-variance analysis',       'sub_ml',       'evaluate'),

    -- Natural Language Processing
    ('skill_tokenization',  'Text Preprocessing',               'Tokenize, normalize, and vectorize text data',                     'sub_nlp',      'apply'),
    ('skill_embeddings',    'Word Embeddings',                  'Use Word2Vec, GloVe, and contextual embeddings',                    'sub_nlp',      'apply'),
    ('skill_transformers',  'Transformer Architecture',         'Explain and implement self-attention and transformer models',       'sub_nlp',      'analyze'),

    -- Data Engineering
    ('skill_sql',           'SQL',                              'Write complex queries with joins, subqueries, and window functions','sub_dataeng',  'apply'),
    ('skill_etl',           'ETL Pipelines',                    'Design and implement extract-transform-load workflows',             'sub_dataeng',  'apply'),
    ('skill_streaming',     'Stream Processing',                'Build real-time data pipelines with event streaming platforms',      'sub_dataeng',  'apply'),

    -- Frontend Development
    ('skill_html_css',      'HTML & CSS',                       'Write semantic HTML and responsive CSS layouts',                    'sub_frontend', 'apply'),
    ('skill_javascript',    'JavaScript',                       'Write modern ES6+ JavaScript for browser and server environments', 'sub_frontend', 'apply'),
    ('skill_vue',           'Vue.js',                           'Build reactive UIs with Vue 3 Composition API and SFCs',           'sub_frontend', 'apply'),
    ('skill_react',         'React',                            'Build component-based UIs with React hooks and state management',  'sub_frontend', 'apply'),
    ('skill_accessibility', 'Web Accessibility',                'Implement WCAG guidelines and ARIA patterns',                       'sub_frontend', 'evaluate'),

    -- Backend Development
    ('skill_rest_api',      'REST API Design',                  'Design resource-oriented APIs with proper status codes and auth',   'sub_backend',  'create'),
    ('skill_auth',          'Authentication & Authorization',   'Implement JWT, OAuth 2.0, and role-based access control',           'sub_backend',  'apply'),
    ('skill_db_design',     'Database Design',                  'Normalize schemas, design indexes, and manage migrations',          'sub_backend',  'create'),
    ('skill_graphql',       'GraphQL',                          'Design and implement GraphQL schemas, resolvers, and subscriptions','sub_backend',  'apply'),

    -- DevOps & Deployment
    ('skill_docker',        'Docker & Containers',              'Build container images, compose services, and manage registries',   'sub_devops',   'apply'),
    ('skill_ci_cd',         'CI/CD Pipelines',                  'Configure automated build, test, and deploy pipelines',             'sub_devops',   'apply'),
    ('skill_k8s',           'Kubernetes',                       'Deploy and manage containerized apps on Kubernetes clusters',       'sub_devops',   'apply'),

    -- Cryptography
    ('skill_symmetric',     'Symmetric Encryption',             'Apply AES, ChaCha20, and block cipher modes of operation',          'sub_crypto',   'apply'),
    ('skill_asymmetric',    'Asymmetric Encryption',            'Use RSA, elliptic curves, and key exchange protocols',              'sub_crypto',   'apply'),
    ('skill_hash_crypto',   'Cryptographic Hashing',            'Apply SHA-256, BLAKE2, and hash-based data structures',            'sub_crypto',   'apply'),
    ('skill_signatures',    'Digital Signatures',               'Implement and verify Ed25519 and ECDSA signatures',                'sub_crypto',   'apply'),
    ('skill_zk',            'Zero-Knowledge Proofs',            'Explain ZK-SNARKs, ZK-STARKs, and their applications',             'sub_crypto',   'understand'),

    -- Network Security
    ('skill_tls',           'TLS & Certificate Management',     'Configure TLS, manage certificates, and diagnose handshake issues','sub_netsec',   'apply'),
    ('skill_firewalls',     'Firewalls & IDS',                  'Configure firewall rules and intrusion detection systems',          'sub_netsec',   'apply'),

    -- UX Design
    ('skill_user_research', 'User Research',                    'Plan and conduct user interviews, surveys, and usability tests',    'sub_ux',       'evaluate'),
    ('skill_ia',            'Information Architecture',          'Organize content structures, navigation, and labeling systems',     'sub_ux',       'create'),
    ('skill_wireframing',   'Wireframing & Prototyping',        'Create low and high-fidelity wireframes and interactive prototypes','sub_ux',       'create'),

    -- UI Design
    ('skill_color_theory',  'Color Theory',                     'Apply color harmony, contrast, and accessibility standards',        'sub_ui',       'apply'),
    ('skill_typography',    'Typography',                       'Select and pair typefaces, set scales, and manage readability',      'sub_ui',       'apply'),
    ('skill_design_systems','Design Systems',                   'Build and maintain reusable component libraries and tokens',        'sub_ui',       'create');

-- ============================================================
-- SKILL PREREQUISITES (directed edges in the DAG)
-- ============================================================
INSERT INTO skill_prerequisites (skill_id, prerequisite_id) VALUES
    -- Algorithms chain
    ('skill_sorting',       'skill_arrays'),
    ('skill_sorting',       'skill_big_o'),
    ('skill_linked_lists',  'skill_arrays'),
    ('skill_stacks_queues', 'skill_arrays'),
    ('skill_stacks_queues', 'skill_linked_lists'),
    ('skill_trees',         'skill_linked_lists'),
    ('skill_trees',         'skill_stacks_queues'),
    ('skill_graphs',        'skill_trees'),
    ('skill_graphs',        'skill_stacks_queues'),
    ('skill_hashing',       'skill_arrays'),
    ('skill_dp',            'skill_big_o'),
    ('skill_dp',            'skill_arrays'),
    ('skill_greedy',        'skill_big_o'),
    ('skill_greedy',        'skill_sorting'),

    -- OS chain
    ('skill_concurrency',   'skill_processes'),
    ('skill_memory',        'skill_processes'),
    ('skill_filesystems',   'skill_memory'),

    -- Programming chain
    ('skill_functional',    'skill_python'),

    -- Networks chain
    ('skill_http',          'skill_tcp_ip'),
    ('skill_dns',           'skill_tcp_ip'),

    -- Calculus chain
    ('skill_derivatives',   'skill_limits'),
    ('skill_integrals',     'skill_derivatives'),
    ('skill_multivariable', 'skill_integrals'),

    -- Linear Algebra chain
    ('skill_matrices',      'skill_vectors'),
    ('skill_eigenvalues',   'skill_matrices'),
    ('skill_svd',           'skill_eigenvalues'),

    -- Discrete chain
    ('skill_combinatorics', 'skill_sets'),
    ('skill_graph_theory',  'skill_logic'),
    ('skill_graph_theory',  'skill_sets'),

    -- Statistics chain
    ('skill_distributions', 'skill_probability'),
    ('skill_inference',     'skill_distributions'),
    ('skill_regression',    'skill_distributions'),

    -- ML chain
    ('skill_supervised',    'skill_regression'),
    ('skill_supervised',    'skill_matrices'),
    ('skill_unsupervised',  'skill_matrices'),
    ('skill_unsupervised',  'skill_probability'),
    ('skill_neural_nets',   'skill_supervised'),
    ('skill_neural_nets',   'skill_derivatives'),
    ('skill_deep_learning', 'skill_neural_nets'),
    ('skill_ml_eval',       'skill_supervised'),
    ('skill_ml_eval',       'skill_inference'),

    -- NLP chain
    ('skill_embeddings',    'skill_tokenization'),
    ('skill_embeddings',    'skill_vectors'),
    ('skill_transformers',  'skill_embeddings'),
    ('skill_transformers',  'skill_neural_nets'),

    -- Data Engineering chain
    ('skill_etl',           'skill_sql'),
    ('skill_streaming',     'skill_etl'),

    -- Frontend chain
    ('skill_javascript',    'skill_html_css'),
    ('skill_vue',           'skill_javascript'),
    ('skill_react',         'skill_javascript'),
    ('skill_accessibility', 'skill_html_css'),
    ('skill_typescript',    'skill_javascript'),

    -- Backend chain
    ('skill_rest_api',      'skill_http'),
    ('skill_auth',          'skill_rest_api'),
    ('skill_db_design',     'skill_sql'),
    ('skill_graphql',       'skill_rest_api'),

    -- DevOps chain
    ('skill_ci_cd',         'skill_docker'),
    ('skill_k8s',           'skill_docker'),

    -- Crypto chain
    ('skill_asymmetric',    'skill_symmetric'),
    ('skill_hash_crypto',   'skill_symmetric'),
    ('skill_signatures',    'skill_asymmetric'),
    ('skill_signatures',    'skill_hash_crypto'),
    ('skill_zk',            'skill_asymmetric'),

    -- Security chain
    ('skill_tls',           'skill_asymmetric'),
    ('skill_tls',           'skill_hash_crypto'),
    ('skill_firewalls',     'skill_tcp_ip'),

    -- UX chain
    ('skill_wireframing',   'skill_user_research'),
    ('skill_wireframing',   'skill_ia'),

    -- UI chain
    ('skill_design_systems','skill_color_theory'),
    ('skill_design_systems','skill_typography');

-- ============================================================
-- SKILL RELATIONS (undirected cross-topic links)
-- ============================================================
INSERT INTO skill_relations (skill_id, related_skill_id, relation_type) VALUES
    ('skill_graph_theory',  'skill_graphs',         'complementary'),
    ('skill_big_o',         'skill_logic',          'complementary'),
    ('skill_hashing',       'skill_hash_crypto',    'related'),
    ('skill_vue',           'skill_react',          'alternative'),
    ('skill_rest_api',      'skill_graphql',        'alternative'),
    ('skill_docker',        'skill_k8s',            'complementary'),
    ('skill_supervised',    'skill_unsupervised',   'complementary'),
    ('skill_sql',           'skill_db_design',      'complementary'),
    ('skill_color_theory',  'skill_accessibility',  'related'),
    ('skill_typography',    'skill_html_css',       'related'),
    ('skill_user_research', 'skill_accessibility',  'complementary'),
    ('skill_tls',           'skill_firewalls',      'complementary'),
    ('skill_eigenvalues',   'skill_deep_learning',  'complementary'),
    ('skill_probability',   'skill_ml_eval',        'complementary');

-- ============================================================
-- GOVERNANCE DAOs
-- ============================================================
INSERT INTO governance_daos (id, name, description, scope_type, scope_id, status, committee_size, election_interval_days) VALUES
    ('dao_cs',      'Computer Science DAO',     'Governs the Computer Science taxonomy and course quality standards',    'subject_field', 'sf_cs',    'active', 7, 180),
    ('dao_math',    'Mathematics DAO',          'Governs the Mathematics curriculum and proof requirements',             'subject_field', 'sf_math',  'active', 5, 365),
    ('dao_data',    'Data Science DAO',         'Governs Data Science skill standards and model evaluation criteria',    'subject_field', 'sf_data',  'active', 5, 180),
    ('dao_web',     'Web Development DAO',      'Governs web development best practices and technology standards',       'subject_field', 'sf_web',   'active', 5, 180),
    ('dao_cyber',   'Cybersecurity DAO',        'Governs security standards, ethical guidelines, and certification',     'subject_field', 'sf_cyber', 'active', 5, 365),
    ('dao_design',  'Design DAO',               'Governs design quality standards and accessibility requirements',       'subject_field', 'sf_design','active', 5, 365);

-- ============================================================
-- SAMPLE COURSES (author_address is placeholder — will show as "Unknown")
-- ============================================================
INSERT INTO courses (id, title, description, author_address, tags, skill_ids, status) VALUES
    ('course_algo_101',
     'Algorithms & Data Structures 101',
     'A comprehensive introduction to fundamental algorithms and data structures. Covers arrays, linked lists, trees, graphs, sorting, searching, and complexity analysis.',
     'addr_seed_author_1',
     '["algorithms","data-structures","computer-science","fundamentals"]',
     '["skill_big_o","skill_arrays","skill_linked_lists","skill_stacks_queues","skill_trees","skill_sorting","skill_hashing"]',
     'published'),

    ('course_web_fullstack',
     'Full-Stack Web Development with Vue & Rust',
     'Build modern web applications from scratch using Vue 3 on the frontend and Rust on the backend. Covers HTML/CSS, JavaScript, TypeScript, Vue Composition API, REST APIs, and database design.',
     'addr_seed_author_1',
     '["web","vue","rust","fullstack","api"]',
     '["skill_html_css","skill_javascript","skill_typescript","skill_vue","skill_rest_api","skill_db_design","skill_auth"]',
     'published'),

    ('course_ml_foundations',
     'Machine Learning Foundations',
     'From linear regression to neural networks — build a solid ML foundation. Requires basic linear algebra and probability.',
     'addr_seed_author_2',
     '["machine-learning","AI","data-science","neural-networks"]',
     '["skill_regression","skill_supervised","skill_unsupervised","skill_neural_nets","skill_ml_eval"]',
     'published'),

    ('course_crypto_101',
     'Applied Cryptography',
     'Understand the building blocks of modern cryptography: symmetric encryption, public-key systems, hashing, digital signatures, and zero-knowledge proofs.',
     'addr_seed_author_2',
     '["cryptography","security","encryption","blockchain"]',
     '["skill_symmetric","skill_asymmetric","skill_hash_crypto","skill_signatures","skill_zk"]',
     'published'),

    ('course_ux_design',
     'UX Design: Research to Prototype',
     'Learn the full UX design process from user research through wireframing and prototyping. Includes design systems and accessibility.',
     'addr_seed_author_3',
     '["design","UX","UI","accessibility","research"]',
     '["skill_user_research","skill_ia","skill_wireframing","skill_color_theory","skill_typography","skill_design_systems"]',
     'published'),

    ('course_math_discrete',
     'Discrete Mathematics for Computing',
     'A practical discrete math course for software and data workflows: logic, set theory, combinatorics, graph fundamentals, and probability intuition.',
     'addr_seed_author_4',
     '["mathematics","discrete-math","proofs","logic","probability"]',
     '["skill_logic","skill_sets","skill_combinatorics","skill_graph_theory","skill_probability"]',
     'published');

-- ============================================================
-- CHAPTERS (for the algorithms course)
-- ============================================================
INSERT INTO course_chapters (id, course_id, title, description, position) VALUES
    ('ch_algo_1', 'course_algo_101', 'Complexity Analysis',         'Big-O notation and asymptotic analysis',                       0),
    ('ch_algo_2', 'course_algo_101', 'Linear Data Structures',     'Arrays, linked lists, stacks, and queues',                      1),
    ('ch_algo_3', 'course_algo_101', 'Trees & Graphs',             'Binary trees, BSTs, graph representations, and traversals',     2),
    ('ch_algo_4', 'course_algo_101', 'Sorting & Searching',        'Comparison sorts, linear-time sorts, and binary search',        3),
    ('ch_algo_5', 'course_algo_101', 'Hash Tables',                'Hash functions, collision resolution, and applications',         4);

-- CHAPTERS (for the web development course)
INSERT INTO course_chapters (id, course_id, title, description, position) VALUES
    ('ch_web_1',  'course_web_fullstack', 'HTML & CSS Fundamentals',     'Semantic markup, flexbox, grid, and responsive design',     0),
    ('ch_web_2',  'course_web_fullstack', 'JavaScript Essentials',       'ES6+ features, DOM manipulation, and async patterns',      1),
    ('ch_web_3',  'course_web_fullstack', 'TypeScript Deep Dive',        'Type system, generics, and strict configuration',          2),
    ('ch_web_4',  'course_web_fullstack', 'Vue 3 & Composition API',    'Reactivity, composables, routing, and state management',   3),
    ('ch_web_5',  'course_web_fullstack', 'Backend with Rust',           'REST APIs, database layer, auth, and deployment',          4);

-- CHAPTERS (for the ML course)
INSERT INTO course_chapters (id, course_id, title, description, position) VALUES
    ('ch_ml_1',   'course_ml_foundations', 'Regression',                  'Linear and logistic regression from scratch',              0),
    ('ch_ml_2',   'course_ml_foundations', 'Classification & Clustering', 'SVMs, decision trees, k-means, and DBSCAN',               1),
    ('ch_ml_3',   'course_ml_foundations', 'Neural Networks',             'Perceptrons, backpropagation, and deep architectures',     2),
    ('ch_ml_4',   'course_ml_foundations', 'Model Evaluation',            'Cross-validation, metrics, and bias-variance tradeoff',    3);

-- CHAPTERS (for the crypto course)
INSERT INTO course_chapters (id, course_id, title, description, position) VALUES
    ('ch_cry_1',  'course_crypto_101', 'Symmetric Cryptography',     'Block ciphers, AES, modes of operation, and stream ciphers',   0),
    ('ch_cry_2',  'course_crypto_101', 'Public-Key Cryptography',    'RSA, Diffie-Hellman, elliptic curves',                         1),
    ('ch_cry_3',  'course_crypto_101', 'Hashing & Signatures',       'SHA-256, BLAKE2, Ed25519, and merkle trees',                   2),
    ('ch_cry_4',  'course_crypto_101', 'Zero-Knowledge Proofs',      'ZK-SNARKs, ZK-STARKs, and privacy applications',               3);

-- CHAPTERS (for the UX design course)
INSERT INTO course_chapters (id, course_id, title, description, position) VALUES
    ('ch_ux_1',   'course_ux_design', 'User Research Methods',       'Interviews, surveys, personas, and journey maps',              0),
    ('ch_ux_2',   'course_ux_design', 'Information Architecture',    'Sitemaps, card sorting, and navigation patterns',              1),
    ('ch_ux_3',   'course_ux_design', 'Wireframing & Prototyping',   'From sketches to interactive prototypes',                      2),
    ('ch_ux_4',   'course_ux_design', 'Visual Design Fundamentals',  'Color, typography, spacing, and design tokens',                3);

-- CHAPTERS (for the discrete math course)
INSERT INTO course_chapters (id, course_id, title, description, position) VALUES
    ('ch_math_1', 'course_math_discrete', 'Logic & Proof Basics',      'Propositions, implications, predicates, and proof patterns',      0),
    ('ch_math_2', 'course_math_discrete', 'Sets & Combinatorics',      'Set operations, counting principles, permutations, combinations',  1),
    ('ch_math_3', 'course_math_discrete', 'Graph Foundations',          'Graph terminology, traversals, and modeling real systems',         2),
    ('ch_math_4', 'course_math_discrete', 'Probability for Engineers',  'Random variables, expected value, and practical probability',      3);

-- ============================================================
-- ELEMENTS (fair representation of all element types)
-- Types: text, quiz, video, pdf, downloadable, assessment,
--        objective_single_mcq, objective_multi_mcq,
--        subjective_mcq, essay, interactive
-- ============================================================
INSERT INTO course_elements (id, chapter_id, title, element_type, position, duration_seconds) VALUES
    -- Algo course - Chapter 1: Complexity Analysis
    ('el_algo_1_1', 'ch_algo_1', 'What is Big-O?',                       'text',  0, NULL),
    ('el_algo_1_2', 'ch_algo_1', 'Analyzing Loops',                      'text',  1, NULL),
    ('el_algo_1_3', 'ch_algo_1', 'Complexity Quiz',                      'quiz',  2, NULL),
    ('el_algo_1_4', 'ch_algo_1', 'Complexity Lecture Video',              'video', 3, 720),
    -- Algo course - Chapter 2: Linear Data Structures
    ('el_algo_2_1', 'ch_algo_2', 'Array Operations',                     'text',  0, NULL),
    ('el_algo_2_2', 'ch_algo_2', 'Linked List Implementation',           'text',  1, NULL),
    ('el_algo_2_3', 'ch_algo_2', 'Stack & Queue Patterns',               'text',  2, NULL),
    ('el_algo_2_4', 'ch_algo_2', 'Data Structures Quiz',                 'quiz',  3, NULL),
    ('el_algo_2_5', 'ch_algo_2', 'Array vs Linked List Trade-offs',      'objective_single_mcq', 4, NULL),
    -- Algo course - Chapter 3: Trees & Graphs
    ('el_algo_3_1', 'ch_algo_3', 'Binary Trees Explained',               'text',  0, NULL),
    ('el_algo_3_2', 'ch_algo_3', 'Graph Representations',                'text',  1, NULL),
    ('el_algo_3_3', 'ch_algo_3', 'BFS vs DFS',                           'text',  2, NULL),
    ('el_algo_3_4', 'ch_algo_3', 'Trees & Graphs Quiz',                  'quiz',  3, NULL),
    ('el_algo_3_5', 'ch_algo_3', 'Graph Traversal Simulation',           'interactive', 4, NULL),
    -- Algo course - Chapter 4: Sorting & Searching
    ('el_algo_4_1', 'ch_algo_4', 'Bubble Sort & Selection Sort',         'text',  0, NULL),
    ('el_algo_4_2', 'ch_algo_4', 'Merge Sort & Quick Sort',              'text',  1, NULL),
    ('el_algo_4_3', 'ch_algo_4', 'Sorting Quiz',                         'quiz',  2, NULL),
    ('el_algo_4_4', 'ch_algo_4', 'Sorting Algorithm Comparison',         'objective_multi_mcq', 3, NULL),
    -- Algo course - Chapter 5: Hash Tables
    ('el_algo_5_1', 'ch_algo_5', 'Hash Functions',                       'text',  0, NULL),
    ('el_algo_5_2', 'ch_algo_5', 'Collision Resolution',                 'text',  1, NULL),
    ('el_algo_5_3', 'ch_algo_5', 'Hash Tables Quiz',                     'quiz',  2, NULL),
    ('el_algo_5_4', 'ch_algo_5', 'Algorithms Final Assessment',          'assessment', 3, NULL),

    -- Web course - Chapter 1: HTML & CSS Fundamentals
    ('el_web_1_1',  'ch_web_1', 'Semantic HTML',                         'text',  0, NULL),
    ('el_web_1_2',  'ch_web_1', 'Flexbox & Grid',                        'text',  1, NULL),
    ('el_web_1_3',  'ch_web_1', 'CSS Layout Workshop',                   'interactive', 2, NULL),
    ('el_web_1_4',  'ch_web_1', 'HTML & CSS Cheat Sheet',                'pdf',   3, NULL),
    -- Web course - Chapter 2: JavaScript Essentials
    ('el_web_2_1',  'ch_web_2', 'ES6+ Features',                         'text',  0, NULL),
    ('el_web_2_2',  'ch_web_2', 'Async/Await Patterns',                  'text',  1, NULL),
    ('el_web_2_3',  'ch_web_2', 'JavaScript Fundamentals Check',         'objective_single_mcq', 2, NULL),
    ('el_web_2_4',  'ch_web_2', 'Build a Todo App',                      'essay', 3, NULL),
    -- Web course - Chapter 3: TypeScript Deep Dive
    ('el_web_3_1',  'ch_web_3', 'TypeScript Type System',                'text',  0, NULL),
    ('el_web_3_2',  'ch_web_3', 'TypeScript Generics Video',             'video', 1, 540),
    -- Web course - Chapter 4: Vue 3 & Composition API
    ('el_web_4_1',  'ch_web_4', 'Vue Reactivity System',                 'text',  0, NULL),
    ('el_web_4_2',  'ch_web_4', 'Composables Pattern',                   'text',  1, NULL),
    ('el_web_4_3',  'ch_web_4', 'Vue Component Design',                  'subjective_mcq', 2, NULL),
    -- Web course - Chapter 5: Backend with Rust
    ('el_web_5_1',  'ch_web_5', 'Building REST APIs in Rust',            'text',  0, NULL),
    ('el_web_5_2',  'ch_web_5', 'Database Design & Migrations',          'text',  1, NULL),
    ('el_web_5_3',  'ch_web_5', 'Authentication with JWT',               'text',  2, NULL),
    ('el_web_5_4',  'ch_web_5', 'Starter Project Template',              'downloadable', 3, NULL),
    ('el_web_5_5',  'ch_web_5', 'Full-Stack Web Dev Final Assessment',   'assessment', 4, NULL),

    -- ML course - Chapter 1: Regression
    ('el_ml_1_1',   'ch_ml_1', 'Linear Regression from Scratch',         'text',  0, NULL),
    ('el_ml_1_2',   'ch_ml_1', 'Logistic Regression',                    'text',  1, NULL),
    ('el_ml_1_3',   'ch_ml_1', 'Regression Intuition Video',             'video', 2, 600),
    -- ML course - Chapter 2: Classification & Clustering
    ('el_ml_2_1',   'ch_ml_2', 'Decision Trees & Random Forests',        'text',  0, NULL),
    ('el_ml_2_2',   'ch_ml_2', 'K-Means Clustering',                     'text',  1, NULL),
    ('el_ml_2_3',   'ch_ml_2', 'Classify or Cluster?',                   'objective_multi_mcq', 2, NULL),
    ('el_ml_2_4',   'ch_ml_2', 'K-Means Interactive Visualization',      'interactive', 3, NULL),
    -- ML course - Chapter 3: Neural Networks
    ('el_ml_3_1',   'ch_ml_3', 'Neural Network Architecture',            'text',  0, NULL),
    ('el_ml_3_2',   'ch_ml_3', 'Backpropagation',                        'text',  1, NULL),
    ('el_ml_3_3',   'ch_ml_3', 'Design a Neural Network',                'essay', 2, NULL),
    -- ML course - Chapter 4: Model Evaluation
    ('el_ml_4_1',   'ch_ml_4', 'Cross-Validation Techniques',            'text',  0, NULL),
    ('el_ml_4_2',   'ch_ml_4', 'Evaluation Metrics Quiz',                'quiz',  1, NULL),
    ('el_ml_4_3',   'ch_ml_4', 'ML Foundations Final Assessment',        'assessment', 2, NULL),
    ('el_ml_4_4',   'ch_ml_4', 'ML Research Paper Collection',           'pdf',   3, NULL),

    -- Crypto course - Chapter 1: Symmetric Cryptography
    ('el_cry_1_1',  'ch_cry_1', 'Block Ciphers & AES',                   'text',  0, NULL),
    ('el_cry_1_2',  'ch_cry_1', 'AES Encryption Demo Video',             'video', 1, 480),
    ('el_cry_1_3',  'ch_cry_1', 'AES Mode Selection',                    'objective_single_mcq', 2, NULL),
    -- Crypto course - Chapter 2: Public-Key Cryptography
    ('el_cry_2_1',  'ch_cry_2', 'RSA Explained',                         'text',  0, NULL),
    ('el_cry_2_2',  'ch_cry_2', 'Elliptic Curve Cryptography',           'text',  1, NULL),
    ('el_cry_2_3',  'ch_cry_2', 'RSA vs ECC Trade-offs',                 'subjective_mcq', 2, NULL),
    -- Crypto course - Chapter 3: Hashing & Signatures
    ('el_cry_3_1',  'ch_cry_3', 'SHA-256 & BLAKE2',                      'text',  0, NULL),
    ('el_cry_3_2',  'ch_cry_3', 'Digital Signatures with Ed25519',       'text',  1, NULL),
    ('el_cry_3_3',  'ch_cry_3', 'Hash Function Properties',              'objective_multi_mcq', 2, NULL),
    ('el_cry_3_4',  'ch_cry_3', 'Crypto Toolkit Cheat Sheet',            'downloadable', 3, NULL),
    -- Crypto course - Chapter 4: Zero-Knowledge Proofs
    ('el_cry_4_1',  'ch_cry_4', 'Introduction to ZK Proofs',             'text',  0, NULL),
    ('el_cry_4_2',  'ch_cry_4', 'ZK Proof Interactive Demo',             'interactive', 1, NULL),
    ('el_cry_4_3',  'ch_cry_4', 'Cryptography Final Assessment',         'assessment', 2, NULL),

    -- UX course - Chapter 1: User Research Methods
    ('el_ux_1_1',   'ch_ux_1', 'Planning User Interviews',               'text',  0, NULL),
    ('el_ux_1_2',   'ch_ux_1', 'Creating Personas',                      'text',  1, NULL),
    ('el_ux_1_3',   'ch_ux_1', 'User Research Methods Video',            'video', 2, 660),
    ('el_ux_1_4',   'ch_ux_1', 'Research Plan Essay',                    'essay', 3, NULL),
    -- UX course - Chapter 2: Information Architecture
    ('el_ux_2_1',   'ch_ux_2', 'Card Sorting Workshop',                  'text',  0, NULL),
    ('el_ux_2_2',   'ch_ux_2', 'Navigation Patterns',                    'text',  1, NULL),
    ('el_ux_2_3',   'ch_ux_2', 'IA Best Practices',                      'objective_single_mcq', 2, NULL),
    -- UX course - Chapter 3: Wireframing & Prototyping
    ('el_ux_3_1',   'ch_ux_3', 'Low-Fidelity Wireframes',                'text',  0, NULL),
    ('el_ux_3_2',   'ch_ux_3', 'Interactive Prototyping',                 'text',  1, NULL),
    ('el_ux_3_3',   'ch_ux_3', 'Wireframe Templates',                    'downloadable', 2, NULL),
    ('el_ux_3_4',   'ch_ux_3', 'Prototype Fidelity Levels',              'subjective_mcq', 3, NULL),
    -- UX course - Chapter 4: Visual Design Fundamentals
    ('el_ux_4_1',   'ch_ux_4', 'Color Theory for Screens',               'text',  0, NULL),
    ('el_ux_4_2',   'ch_ux_4', 'Typography Best Practices',              'text',  1, NULL),
    ('el_ux_4_3',   'ch_ux_4', 'Design System Reference',                'pdf',   2, NULL),
    ('el_ux_4_4',   'ch_ux_4', 'UX Design Final Assessment',             'assessment', 3, NULL),

    -- Discrete Math course
    ('el_math_1_1', 'ch_math_1', 'Propositional Logic Essentials',       'text',  0, NULL),
    ('el_math_1_2', 'ch_math_1', 'Proof Strategies in Practice',         'text',  1, NULL),
    ('el_math_1_3', 'ch_math_1', 'Logic Foundations Quiz',               'quiz',  2, NULL),
    ('el_math_2_1', 'ch_math_2', 'Set Operations & Counting Rules',      'text',  0, NULL),
    ('el_math_2_2', 'ch_math_2', 'Choosing Counting Techniques',         'objective_single_mcq', 1, NULL),
    ('el_math_3_1', 'ch_math_3', 'Graph Models for Real Systems',        'text',  0, NULL),
    ('el_math_3_2', 'ch_math_3', 'Graph Thinking Interactive',           'interactive', 1, NULL),
    ('el_math_4_1', 'ch_math_4', 'Expected Value & Risk',                'text',  0, NULL),
    ('el_math_4_2', 'ch_math_4', 'Discrete Math Final Assessment',       'assessment', 1, NULL);

-- ============================================================
-- ELEMENT SKILL TAGS (link elements to skills for evidence)
-- ============================================================
INSERT INTO element_skill_tags (element_id, skill_id, weight) VALUES
    -- Algo course
    ('el_algo_1_1', 'skill_big_o',          1.0),
    ('el_algo_1_2', 'skill_big_o',          1.0),
    ('el_algo_1_3', 'skill_big_o',          1.0),
    ('el_algo_1_4', 'skill_big_o',          0.5),
    ('el_algo_2_1', 'skill_arrays',         1.0),
    ('el_algo_2_2', 'skill_linked_lists',   1.0),
    ('el_algo_2_3', 'skill_stacks_queues',  1.0),
    ('el_algo_2_4', 'skill_arrays',         0.5),
    ('el_algo_2_4', 'skill_linked_lists',   0.5),
    ('el_algo_2_4', 'skill_stacks_queues',  0.5),
    ('el_algo_2_5', 'skill_arrays',         1.0),
    ('el_algo_2_5', 'skill_linked_lists',   0.5),
    ('el_algo_3_1', 'skill_trees',          1.0),
    ('el_algo_3_2', 'skill_graphs',         1.0),
    ('el_algo_3_3', 'skill_graphs',         1.0),
    ('el_algo_3_4', 'skill_trees',          0.5),
    ('el_algo_3_4', 'skill_graphs',         0.5),
    ('el_algo_3_5', 'skill_graphs',         1.0),
    ('el_algo_3_5', 'skill_trees',          0.5),
    ('el_algo_4_1', 'skill_sorting',        1.0),
    ('el_algo_4_2', 'skill_sorting',        1.0),
    ('el_algo_4_3', 'skill_sorting',        1.0),
    ('el_algo_4_4', 'skill_sorting',        1.0),
    ('el_algo_5_1', 'skill_hashing',        1.0),
    ('el_algo_5_2', 'skill_hashing',        1.0),
    ('el_algo_5_3', 'skill_hashing',        1.0),
    ('el_algo_5_4', 'skill_big_o',          0.5),
    ('el_algo_5_4', 'skill_arrays',         0.5),
    ('el_algo_5_4', 'skill_trees',          0.5),
    ('el_algo_5_4', 'skill_sorting',        0.5),
    ('el_algo_5_4', 'skill_hashing',        0.5),

    -- Web course
    ('el_web_1_1',  'skill_html_css',       1.0),
    ('el_web_1_2',  'skill_html_css',       1.0),
    ('el_web_1_3',  'skill_html_css',       1.0),
    ('el_web_1_4',  'skill_html_css',       0.5),
    ('el_web_2_1',  'skill_javascript',     1.0),
    ('el_web_2_2',  'skill_javascript',     1.0),
    ('el_web_2_3',  'skill_javascript',     1.0),
    ('el_web_2_4',  'skill_javascript',     1.0),
    ('el_web_2_4',  'skill_html_css',       0.5),
    ('el_web_3_1',  'skill_typescript',     1.0),
    ('el_web_3_2',  'skill_typescript',     1.0),
    ('el_web_4_1',  'skill_vue',            1.0),
    ('el_web_4_2',  'skill_vue',            1.0),
    ('el_web_4_3',  'skill_vue',            1.0),
    ('el_web_5_1',  'skill_rest_api',       1.0),
    ('el_web_5_1',  'skill_rust',           0.5),
    ('el_web_5_2',  'skill_db_design',      1.0),
    ('el_web_5_3',  'skill_auth',           1.0),
    ('el_web_5_4',  'skill_rest_api',       0.5),
    ('el_web_5_4',  'skill_rust',           0.5),
    ('el_web_5_5',  'skill_html_css',       0.5),
    ('el_web_5_5',  'skill_javascript',     0.5),
    ('el_web_5_5',  'skill_vue',            0.5),
    ('el_web_5_5',  'skill_rest_api',       0.5),
    ('el_web_5_5',  'skill_db_design',      0.5),

    -- ML course
    ('el_ml_1_1',   'skill_regression',     1.0),
    ('el_ml_1_2',   'skill_regression',     1.0),
    ('el_ml_1_3',   'skill_regression',     0.5),
    ('el_ml_2_1',   'skill_supervised',     1.0),
    ('el_ml_2_2',   'skill_unsupervised',   1.0),
    ('el_ml_2_3',   'skill_supervised',     0.5),
    ('el_ml_2_3',   'skill_unsupervised',   0.5),
    ('el_ml_2_4',   'skill_unsupervised',   1.0),
    ('el_ml_3_1',   'skill_neural_nets',    1.0),
    ('el_ml_3_2',   'skill_neural_nets',    1.0),
    ('el_ml_3_3',   'skill_neural_nets',    1.0),
    ('el_ml_3_3',   'skill_deep_learning',  0.5),
    ('el_ml_4_1',   'skill_ml_eval',        1.0),
    ('el_ml_4_2',   'skill_ml_eval',        1.0),
    ('el_ml_4_3',   'skill_regression',     0.5),
    ('el_ml_4_3',   'skill_supervised',     0.5),
    ('el_ml_4_3',   'skill_neural_nets',    0.5),
    ('el_ml_4_3',   'skill_ml_eval',        0.5),
    ('el_ml_4_4',   'skill_ml_eval',        0.5),

    -- Crypto course
    ('el_cry_1_1',  'skill_symmetric',      1.0),
    ('el_cry_1_2',  'skill_symmetric',      0.5),
    ('el_cry_1_3',  'skill_symmetric',      1.0),
    ('el_cry_2_1',  'skill_asymmetric',     1.0),
    ('el_cry_2_2',  'skill_asymmetric',     1.0),
    ('el_cry_2_3',  'skill_asymmetric',     1.0),
    ('el_cry_2_3',  'skill_symmetric',      0.5),
    ('el_cry_3_1',  'skill_hash_crypto',    1.0),
    ('el_cry_3_2',  'skill_signatures',     1.0),
    ('el_cry_3_3',  'skill_hash_crypto',    1.0),
    ('el_cry_3_4',  'skill_symmetric',      0.5),
    ('el_cry_3_4',  'skill_asymmetric',     0.5),
    ('el_cry_3_4',  'skill_hash_crypto',    0.5),
    ('el_cry_4_1',  'skill_zk',            1.0),
    ('el_cry_4_2',  'skill_zk',            1.0),
    ('el_cry_4_3',  'skill_symmetric',      0.5),
    ('el_cry_4_3',  'skill_asymmetric',     0.5),
    ('el_cry_4_3',  'skill_hash_crypto',    0.5),
    ('el_cry_4_3',  'skill_signatures',     0.5),
    ('el_cry_4_3',  'skill_zk',            0.5),

    -- UX course
    ('el_ux_1_1',   'skill_user_research',  1.0),
    ('el_ux_1_2',   'skill_user_research',  1.0),
    ('el_ux_1_3',   'skill_user_research',  0.5),
    ('el_ux_1_4',   'skill_user_research',  1.0),
    ('el_ux_2_1',   'skill_ia',            1.0),
    ('el_ux_2_2',   'skill_ia',            1.0),
    ('el_ux_2_3',   'skill_ia',            1.0),
    ('el_ux_3_1',   'skill_wireframing',    1.0),
    ('el_ux_3_2',   'skill_wireframing',    1.0),
    ('el_ux_3_3',   'skill_wireframing',    0.5),
    ('el_ux_3_4',   'skill_wireframing',    1.0),
    ('el_ux_4_1',   'skill_color_theory',   1.0),
    ('el_ux_4_2',   'skill_typography',     1.0),
    ('el_ux_4_3',   'skill_design_systems', 1.0),
    ('el_ux_4_3',   'skill_color_theory',   0.5),
    ('el_ux_4_3',   'skill_typography',     0.5),
    ('el_ux_4_4',   'skill_user_research',  0.5),
    ('el_ux_4_4',   'skill_ia',            0.5),
    ('el_ux_4_4',   'skill_wireframing',    0.5),
    ('el_ux_4_4',   'skill_color_theory',   0.5),
    ('el_ux_4_4',   'skill_typography',     0.5),

    -- Discrete Math course
    ('el_math_1_1', 'skill_logic',          1.0),
    ('el_math_1_2', 'skill_logic',          1.0),
    ('el_math_1_3', 'skill_logic',          1.0),
    ('el_math_2_1', 'skill_sets',           1.0),
    ('el_math_2_1', 'skill_combinatorics',  0.5),
    ('el_math_2_2', 'skill_combinatorics',  1.0),
    ('el_math_3_1', 'skill_graph_theory',   1.0),
    ('el_math_3_2', 'skill_graph_theory',   1.0),
    ('el_math_4_1', 'skill_probability',    1.0),
    ('el_math_4_2', 'skill_logic',          0.5),
    ('el_math_4_2', 'skill_sets',           0.5),
    ('el_math_4_2', 'skill_combinatorics',  0.5),
    ('el_math_4_2', 'skill_graph_theory',   0.5),
    ('el_math_4_2', 'skill_probability',    0.5);

-- ============================================================
-- SEEDED SKILL PROOFS (ensures earned/available/locked graph states)
-- ============================================================
INSERT INTO skill_proofs (id, skill_id, proficiency_level, confidence, evidence_count) VALUES
    ('proof_001', 'skill_arrays',        'apply',      0.93, 4),
    ('proof_002', 'skill_big_o',         'analyze',    0.88, 3),
    ('proof_003', 'skill_linked_lists',  'apply',      0.90, 3),
    ('proof_004', 'skill_stacks_queues', 'apply',      0.85, 2),
    ('proof_005', 'skill_html_css',      'apply',      0.92, 4),
    ('proof_006', 'skill_javascript',    'apply',      0.89, 3),
    ('proof_007', 'skill_typescript',    'apply',      0.81, 2),
    ('proof_008', 'skill_sql',           'apply',      0.86, 3),
    ('proof_009', 'skill_symmetric',     'apply',      0.83, 2),
    ('proof_010', 'skill_user_research', 'evaluate',   0.84, 2),
    ('proof_011', 'skill_ia',            'create',     0.80, 2),
    ('proof_012', 'skill_wireframing',   'create',     0.82, 2);

"##;

const BACKFILL_SQL: &str = r##"
-- Temporarily disable FK checks during bulk seed insert
PRAGMA foreign_keys = OFF;

-- ============================================================
-- P1: ENROLLMENTS & PROGRESS
-- ============================================================
-- 4 enrollments: 1 completed, 2 active (in-progress), 1 recently started
INSERT INTO enrollments (id, course_id, enrolled_at, completed_at, status) VALUES
    ('enroll_algo',   'course_algo_101',      '2026-01-15T10:00:00', '2026-03-20T16:45:00', 'completed'),
    ('enroll_web',    'course_web_fullstack', '2026-02-01T09:30:00', NULL,                   'active'),
    ('enroll_ml',     'course_ml_foundations','2026-03-10T14:00:00', NULL,                   'active'),
    ('enroll_crypto', 'course_crypto_101',        '2026-04-01T11:15:00', NULL,                   'active');

-- Algo 101: fully completed (all elements done)
INSERT INTO element_progress (id, enrollment_id, element_id, status, score, time_spent, completed_at) VALUES
    ('ep_a1_1', 'enroll_algo', 'el_algo_1_1', 'completed', NULL, 420,  '2026-01-16T11:00:00'),
    ('ep_a1_2', 'enroll_algo', 'el_algo_1_2', 'completed', NULL, 1200, '2026-01-18T14:30:00'),
    ('ep_a1_3', 'enroll_algo', 'el_algo_1_3', 'completed', 0.92, 900,  '2026-01-20T10:15:00'),
    ('ep_a2_1', 'enroll_algo', 'el_algo_2_1', 'completed', NULL, 480,  '2026-01-22T09:00:00'),
    ('ep_a2_2', 'enroll_algo', 'el_algo_2_2', 'completed', NULL, 1500, '2026-01-25T16:00:00'),
    ('ep_a2_3', 'enroll_algo', 'el_algo_2_3', 'completed', 0.88, 1080, '2026-01-28T11:30:00'),
    ('ep_a3_1', 'enroll_algo', 'el_algo_3_1', 'completed', NULL, 600,  '2026-02-01T10:00:00'),
    ('ep_a3_2', 'enroll_algo', 'el_algo_3_2', 'completed', NULL, 1800, '2026-02-05T15:45:00'),
    ('ep_a3_3', 'enroll_algo', 'el_algo_3_3', 'completed', 0.95, 720,  '2026-02-08T10:30:00'),
    ('ep_a4_1', 'enroll_algo', 'el_algo_4_1', 'completed', NULL, 360,  '2026-02-10T09:15:00'),
    ('ep_a4_2', 'enroll_algo', 'el_algo_4_2', 'completed', NULL, 2400, '2026-02-15T14:00:00'),
    ('ep_a4_3', 'enroll_algo', 'el_algo_4_3', 'completed', 0.90, 1200, '2026-03-20T16:45:00');

-- Web fullstack: 60% through (chapters 1-3 done, partway through 4)
INSERT INTO element_progress (id, enrollment_id, element_id, status, score, time_spent, completed_at) VALUES
    ('ep_w1_1', 'enroll_web', 'el_web_1_1', 'completed', NULL, 300,  '2026-02-02T10:00:00'),
    ('ep_w1_2', 'enroll_web', 'el_web_1_2', 'completed', NULL, 900,  '2026-02-04T11:30:00'),
    ('ep_w1_3', 'enroll_web', 'el_web_1_3', 'completed', 0.96, 600,  '2026-02-06T09:45:00'),
    ('ep_w2_1', 'enroll_web', 'el_web_2_1', 'completed', NULL, 480,  '2026-02-08T14:00:00'),
    ('ep_w2_2', 'enroll_web', 'el_web_2_2', 'completed', NULL, 1200, '2026-02-11T10:30:00'),
    ('ep_w2_3', 'enroll_web', 'el_web_2_3', 'completed', 0.84, 900,  '2026-02-14T16:00:00'),
    ('ep_w3_1', 'enroll_web', 'el_web_3_1', 'completed', NULL, 600,  '2026-02-16T09:00:00'),
    ('ep_w3_2', 'enroll_web', 'el_web_3_2', 'completed', NULL, 1500, '2026-02-20T13:15:00'),
    ('ep_w3_3', 'enroll_web', 'el_web_3_3', 'completed', 0.91, 1080, '2026-02-24T11:00:00'),
    ('ep_w4_1', 'enroll_web', 'el_web_4_1', 'completed', NULL, 540,  '2026-03-01T10:00:00'),
    ('ep_w4_2', 'enroll_web', 'el_web_4_2', 'in_progress', NULL, 600, NULL);

-- ML foundations: just started (chapter 1 done)
INSERT INTO element_progress (id, enrollment_id, element_id, status, score, time_spent, completed_at) VALUES
    ('ep_m1_1', 'enroll_ml', 'el_ml_1_1', 'completed',   NULL, 360, '2026-03-12T10:00:00'),
    ('ep_m1_2', 'enroll_ml', 'el_ml_1_2', 'completed',   NULL, 900, '2026-03-14T14:30:00'),
    ('ep_m1_3', 'enroll_ml', 'el_ml_1_3', 'completed',   0.87, 720, '2026-03-16T11:00:00'),
    ('ep_m2_1', 'enroll_ml', 'el_ml_2_1', 'in_progress', NULL, 180, NULL);

-- Crypto: barely started
INSERT INTO element_progress (id, enrollment_id, element_id, status, score, time_spent, completed_at) VALUES
    ('ep_c1_1', 'enroll_crypto', 'el_cry_1_1', 'completed',   NULL, 480, '2026-04-02T10:30:00'),
    ('ep_c1_2', 'enroll_crypto', 'el_cry_1_2', 'in_progress', NULL, 120, NULL);

-- Course notes
INSERT INTO course_notes (id, enrollment_id, chapter_id, element_id, preview_text) VALUES
    ('note_001', 'enroll_algo', 'ch_algo_1', 'el_algo_1_2', 'Key insight: amortized O(1) for dynamic arrays because doubling only happens log(n) times. Think of it like paying a little extra each insertion to cover the rare expensive resize.'),
    ('note_002', 'enroll_web',  'ch_web_2',  'el_web_2_2',  'Vue 3 Composition API vs Options API: use composables for shared stateful logic. defineProps + defineEmits for type-safe component contracts. Remember: ref() for primitives, reactive() for objects.'),
    ('note_003', 'enroll_ml',   'ch_ml_1',   'el_ml_1_2',   'Bias-variance tradeoff: high bias = underfitting (model too simple), high variance = overfitting (model too complex). Cross-validation is the practical tool to detect both.');

-- ============================================================
-- P2: ASSESSMENTS, EVIDENCE RECORDS, & PROOF LINKS
-- ============================================================
-- Skill assessments (one per skill that has a proof)
INSERT INTO skill_assessments (id, skill_id, course_id, assessment_type, proficiency_level, difficulty, trust_factor) VALUES
    ('sa_001', 'skill_arrays',        'course_algo_101',      'quiz',        'apply',    0.60, 1.0),
    ('sa_002', 'skill_big_o',         'course_algo_101',      'quiz',        'analyze',  0.70, 1.0),
    ('sa_003', 'skill_linked_lists',  'course_algo_101',      'quiz',        'apply',    0.55, 1.0),
    ('sa_004', 'skill_stacks_queues', 'course_algo_101',      'quiz',        'apply',    0.55, 1.0),
    ('sa_005', 'skill_html_css',      'course_web_fullstack', 'quiz',        'apply',    0.50, 1.0),
    ('sa_006', 'skill_javascript',    'course_web_fullstack', 'quiz',        'apply',    0.60, 1.0),
    ('sa_007', 'skill_typescript',    'course_web_fullstack', 'quiz',        'apply',    0.65, 1.0),
    ('sa_008', 'skill_sql',           NULL,                   'project',     'apply',    0.70, 1.0),
    ('sa_009', 'skill_symmetric',     'course_crypto_101',        'exam',        'apply',    0.75, 1.0),
    ('sa_010', 'skill_user_research', 'course_ux_design',     'peer_review', 'evaluate', 0.65, 0.9),
    ('sa_011', 'skill_ia',            'course_ux_design',     'project',     'create',   0.70, 1.0),
    ('sa_012', 'skill_wireframing',   'course_ux_design',     'project',     'create',   0.65, 1.0);

-- Evidence records backing each proof (2-4 per proof as claimed by evidence_count)
INSERT INTO evidence_records (id, skill_assessment_id, skill_id, proficiency_level, score, difficulty, trust_factor, course_id, instructor_address, created_at) VALUES
    -- proof_001: skill_arrays (4 evidence)
    ('ev_001a', 'sa_001', 'skill_arrays',   'apply', 0.95, 0.55, 1.0, 'course_algo_101', 'addr_seed_author_1', '2026-01-20T10:15:00'),
    ('ev_001b', 'sa_001', 'skill_arrays',   'apply', 0.90, 0.60, 1.0, 'course_algo_101', 'addr_seed_author_1', '2026-02-08T10:30:00'),
    ('ev_001c', 'sa_001', 'skill_arrays',   'apply', 0.93, 0.65, 1.0, 'course_algo_101', 'addr_seed_author_1', '2026-02-28T14:00:00'),
    ('ev_001d', 'sa_001', 'skill_arrays',   'apply', 0.92, 0.60, 1.0, 'course_algo_101', 'addr_seed_author_1', '2026-03-15T11:30:00'),
    -- proof_002: skill_big_o (3 evidence)
    ('ev_002a', 'sa_002', 'skill_big_o',    'analyze', 0.85, 0.70, 1.0, 'course_algo_101', 'addr_seed_author_1', '2026-01-28T11:30:00'),
    ('ev_002b', 'sa_002', 'skill_big_o',    'analyze', 0.90, 0.72, 1.0, 'course_algo_101', 'addr_seed_author_1', '2026-02-15T14:00:00'),
    ('ev_002c', 'sa_002', 'skill_big_o',    'analyze', 0.88, 0.68, 1.0, 'course_algo_101', 'addr_seed_author_1', '2026-03-20T16:45:00'),
    -- proof_003: skill_linked_lists (3 evidence)
    ('ev_003a', 'sa_003', 'skill_linked_lists', 'apply', 0.88, 0.55, 1.0, 'course_algo_101', 'addr_seed_author_1', '2026-02-01T10:00:00'),
    ('ev_003b', 'sa_003', 'skill_linked_lists', 'apply', 0.92, 0.58, 1.0, 'course_algo_101', 'addr_seed_author_1', '2026-02-15T09:30:00'),
    ('ev_003c', 'sa_003', 'skill_linked_lists', 'apply', 0.90, 0.55, 1.0, 'course_algo_101', 'addr_seed_author_1', '2026-03-05T13:00:00'),
    -- proof_004: skill_stacks_queues (2 evidence)
    ('ev_004a', 'sa_004', 'skill_stacks_queues', 'apply', 0.84, 0.55, 1.0, 'course_algo_101', 'addr_seed_author_1', '2026-02-05T15:45:00'),
    ('ev_004b', 'sa_004', 'skill_stacks_queues', 'apply', 0.86, 0.58, 1.0, 'course_algo_101', 'addr_seed_author_1', '2026-02-20T10:15:00'),
    -- proof_005: skill_html_css (4 evidence)
    ('ev_005a', 'sa_005', 'skill_html_css', 'apply', 0.94, 0.48, 1.0, 'course_web_fullstack', 'addr_seed_author_1', '2026-02-06T09:45:00'),
    ('ev_005b', 'sa_005', 'skill_html_css', 'apply', 0.91, 0.52, 1.0, 'course_web_fullstack', 'addr_seed_author_1', '2026-02-14T16:00:00'),
    ('ev_005c', 'sa_005', 'skill_html_css', 'apply', 0.90, 0.50, 1.0, 'course_web_fullstack', 'addr_seed_author_1', '2026-02-24T11:00:00'),
    ('ev_005d', 'sa_005', 'skill_html_css', 'apply', 0.93, 0.55, 1.0, 'course_web_fullstack', 'addr_seed_author_1', '2026-03-10T14:30:00'),
    -- proof_006: skill_javascript (3 evidence)
    ('ev_006a', 'sa_006', 'skill_javascript', 'apply', 0.88, 0.58, 1.0, 'course_web_fullstack', 'addr_seed_author_1', '2026-02-14T16:00:00'),
    ('ev_006b', 'sa_006', 'skill_javascript', 'apply', 0.91, 0.62, 1.0, 'course_web_fullstack', 'addr_seed_author_1', '2026-02-24T11:00:00'),
    ('ev_006c', 'sa_006', 'skill_javascript', 'apply', 0.87, 0.60, 1.0, 'course_web_fullstack', 'addr_seed_author_1', '2026-03-15T10:00:00'),
    -- proof_007: skill_typescript (2 evidence)
    ('ev_007a', 'sa_007', 'skill_typescript', 'apply', 0.80, 0.65, 1.0, 'course_web_fullstack', 'addr_seed_author_1', '2026-03-01T10:00:00'),
    ('ev_007b', 'sa_007', 'skill_typescript', 'apply', 0.82, 0.68, 1.0, 'course_web_fullstack', 'addr_seed_author_1', '2026-03-20T14:15:00'),
    -- proof_008: skill_sql (3 evidence)
    ('ev_008a', 'sa_008', 'skill_sql', 'apply', 0.84, 0.68, 1.0, NULL, 'addr_seed_author_2', '2026-01-10T09:00:00'),
    ('ev_008b', 'sa_008', 'skill_sql', 'apply', 0.88, 0.72, 1.0, NULL, 'addr_seed_author_2', '2026-02-05T11:30:00'),
    ('ev_008c', 'sa_008', 'skill_sql', 'apply', 0.86, 0.70, 1.0, NULL, 'addr_seed_author_2', '2026-03-01T15:00:00'),
    -- proof_009: skill_symmetric (2 evidence)
    ('ev_009a', 'sa_009', 'skill_symmetric', 'apply', 0.82, 0.75, 1.0, 'course_crypto_101', 'addr_seed_author_2', '2026-02-20T10:00:00'),
    ('ev_009b', 'sa_009', 'skill_symmetric', 'apply', 0.84, 0.78, 1.0, 'course_crypto_101', 'addr_seed_author_2', '2026-03-10T13:45:00'),
    -- proof_010: skill_user_research (2 evidence)
    ('ev_010a', 'sa_010', 'skill_user_research', 'evaluate', 0.83, 0.65, 0.9, 'course_ux_design', 'addr_seed_author_3', '2026-01-25T14:00:00'),
    ('ev_010b', 'sa_010', 'skill_user_research', 'evaluate', 0.85, 0.68, 0.9, 'course_ux_design', 'addr_seed_author_3', '2026-02-15T10:30:00'),
    -- proof_011: skill_ia (2 evidence)
    ('ev_011a', 'sa_011', 'skill_ia', 'create', 0.78, 0.70, 1.0, 'course_ux_design', 'addr_seed_author_3', '2026-02-01T09:00:00'),
    ('ev_011b', 'sa_011', 'skill_ia', 'create', 0.82, 0.72, 1.0, 'course_ux_design', 'addr_seed_author_3', '2026-03-01T11:15:00'),
    -- proof_012: skill_wireframing (2 evidence)
    ('ev_012a', 'sa_012', 'skill_wireframing', 'create', 0.80, 0.65, 1.0, 'course_ux_design', 'addr_seed_author_3', '2026-02-10T13:00:00'),
    ('ev_012b', 'sa_012', 'skill_wireframing', 'create', 0.84, 0.68, 1.0, 'course_ux_design', 'addr_seed_author_3', '2026-03-05T10:45:00');

-- Link evidence to proofs
INSERT INTO skill_proof_evidence (proof_id, evidence_id) VALUES
    ('proof_001', 'ev_001a'), ('proof_001', 'ev_001b'), ('proof_001', 'ev_001c'), ('proof_001', 'ev_001d'),
    ('proof_002', 'ev_002a'), ('proof_002', 'ev_002b'), ('proof_002', 'ev_002c'),
    ('proof_003', 'ev_003a'), ('proof_003', 'ev_003b'), ('proof_003', 'ev_003c'),
    ('proof_004', 'ev_004a'), ('proof_004', 'ev_004b'),
    ('proof_005', 'ev_005a'), ('proof_005', 'ev_005b'), ('proof_005', 'ev_005c'), ('proof_005', 'ev_005d'),
    ('proof_006', 'ev_006a'), ('proof_006', 'ev_006b'), ('proof_006', 'ev_006c'),
    ('proof_007', 'ev_007a'), ('proof_007', 'ev_007b'),
    ('proof_008', 'ev_008a'), ('proof_008', 'ev_008b'), ('proof_008', 'ev_008c'),
    ('proof_009', 'ev_009a'), ('proof_009', 'ev_009b'),
    ('proof_010', 'ev_010a'), ('proof_010', 'ev_010b'),
    ('proof_011', 'ev_011a'), ('proof_011', 'ev_011b'),
    ('proof_012', 'ev_012a'), ('proof_012', 'ev_012b');

-- ============================================================
-- P3: REPUTATION ASSERTIONS & IMPACT DATA
-- ============================================================
-- 3 instructors with reputation across different domains
INSERT INTO reputation_assertions (id, actor_address, role, skill_id, proficiency_level, score, evidence_count, median_impact, impact_p25, impact_p75, learner_count, impact_variance, window_start, window_end, computation_spec) VALUES
    -- Author 1: Algorithms & Web instructor
    ('rep_001', 'addr_seed_author_1', 'instructor', 'skill_arrays',    'apply',    0.91, 12, 0.08, 0.05, 0.12, 8, 0.003, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_002', 'addr_seed_author_1', 'instructor', 'skill_big_o',     'analyze',  0.87, 9,  0.07, 0.04, 0.10, 6, 0.004, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_003', 'addr_seed_author_1', 'instructor', 'skill_html_css',  'apply',    0.93, 15, 0.09, 0.06, 0.13, 10, 0.002, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_004', 'addr_seed_author_1', 'instructor', 'skill_javascript','apply',    0.89, 11, 0.07, 0.04, 0.11, 7, 0.003, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    -- Author 2: Data & Crypto instructor
    ('rep_005', 'addr_seed_author_2', 'instructor', 'skill_sql',       'apply',    0.85, 8,  0.06, 0.03, 0.09, 5, 0.005, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_006', 'addr_seed_author_2', 'instructor', 'skill_symmetric', 'apply',    0.82, 6,  0.05, 0.02, 0.08, 4, 0.006, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_007', 'addr_seed_author_2', 'instructor', 'skill_supervised','apply',    0.88, 10, 0.08, 0.05, 0.11, 7, 0.004, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    -- Author 3: Design instructor
    ('rep_008', 'addr_seed_author_3', 'instructor', 'skill_user_research', 'evaluate', 0.86, 7, 0.06, 0.03, 0.10, 5, 0.005, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_009', 'addr_seed_author_3', 'instructor', 'skill_ia',            'create',   0.84, 6, 0.05, 0.03, 0.08, 4, 0.004, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_010', 'addr_seed_author_3', 'instructor', 'skill_wireframing',   'create',   0.83, 5, 0.05, 0.02, 0.07, 4, 0.006, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2');

-- Link reputation to proofs
INSERT INTO reputation_evidence (assertion_id, proof_id, delta_confidence, attribution_weight) VALUES
    ('rep_001', 'proof_001', 0.08, 1.0),
    ('rep_002', 'proof_002', 0.07, 1.0),
    ('rep_003', 'proof_005', 0.09, 1.0),
    ('rep_004', 'proof_006', 0.07, 1.0),
    ('rep_005', 'proof_008', 0.06, 1.0),
    ('rep_006', 'proof_009', 0.05, 1.0),
    ('rep_008', 'proof_010', 0.06, 0.9),
    ('rep_009', 'proof_011', 0.05, 1.0),
    ('rep_010', 'proof_012', 0.05, 1.0);

-- Impact deltas (sample per-learner contributions)
INSERT INTO reputation_impact_deltas (id, assertion_id, learner_address, delta, attribution, proof_id) VALUES
    ('rid_001', 'rep_001', 'addr_seed_learner_1', 0.08, 1.0, 'proof_001'),
    ('rid_002', 'rep_001', 'addr_seed_learner_2', 0.06, 1.0, NULL),
    ('rid_003', 'rep_001', 'addr_seed_learner_3', 0.12, 1.0, NULL),
    ('rid_004', 'rep_002', 'addr_seed_learner_1', 0.07, 1.0, 'proof_002'),
    ('rid_005', 'rep_002', 'addr_seed_learner_4', 0.05, 1.0, NULL),
    ('rid_006', 'rep_003', 'addr_seed_learner_1', 0.09, 1.0, 'proof_005'),
    ('rid_007', 'rep_003', 'addr_seed_learner_5', 0.11, 1.0, NULL),
    ('rid_008', 'rep_005', 'addr_seed_learner_1', 0.06, 1.0, 'proof_008');

-- ============================================================
-- P4: GOVERNANCE (members, elections, proposals, votes)
-- ============================================================
-- DAO members (committee members for each DAO)
INSERT INTO governance_dao_members (dao_id, stake_address, role) VALUES
    -- CS DAO: 7 members
    ('dao_cs', 'addr_seed_author_1',   'chair'),
    ('dao_cs', 'addr_seed_author_2',   'committee'),
    ('dao_cs', 'addr_seed_member_1',   'committee'),
    ('dao_cs', 'addr_seed_member_2',   'committee'),
    ('dao_cs', 'addr_seed_member_3',   'member'),
    ('dao_cs', 'addr_seed_member_4',   'member'),
    ('dao_cs', 'addr_seed_member_5',   'member'),
    -- Math DAO: 5 members
    ('dao_math', 'addr_seed_author_2', 'chair'),
    ('dao_math', 'addr_seed_member_1', 'committee'),
    ('dao_math', 'addr_seed_member_6', 'committee'),
    ('dao_math', 'addr_seed_member_7', 'member'),
    ('dao_math', 'addr_seed_member_8', 'member'),
    -- Web DAO: 5 members
    ('dao_web', 'addr_seed_author_1',  'chair'),
    ('dao_web', 'addr_seed_member_2',  'committee'),
    ('dao_web', 'addr_seed_member_9',  'committee'),
    ('dao_web', 'addr_seed_member_10', 'member'),
    ('dao_web', 'addr_seed_member_11', 'member'),
    -- Design DAO: 5 members
    ('dao_design', 'addr_seed_author_3', 'chair'),
    ('dao_design', 'addr_seed_member_3',  'committee'),
    ('dao_design', 'addr_seed_member_12', 'committee'),
    ('dao_design', 'addr_seed_member_13', 'member'),
    ('dao_design', 'addr_seed_member_14', 'member'),
    -- Cyber DAO: 5 members
    ('dao_cyber', 'addr_seed_author_2', 'chair'),
    ('dao_cyber', 'addr_seed_member_4', 'committee'),
    ('dao_cyber', 'addr_seed_member_15', 'member'),
    ('dao_cyber', 'addr_seed_member_16', 'member'),
    ('dao_cyber', 'addr_seed_member_17', 'member'),
    -- Data DAO: 5 members
    ('dao_data', 'addr_seed_author_2', 'chair'),
    ('dao_data', 'addr_seed_member_5', 'committee'),
    ('dao_data', 'addr_seed_member_6', 'committee'),
    ('dao_data', 'addr_seed_member_18', 'member'),
    ('dao_data', 'addr_seed_member_19', 'member');

-- Elections: 1 finalized, 1 in voting phase, 1 in nomination phase
INSERT INTO governance_elections (id, dao_id, title, description, phase, seats, nominee_min_proficiency, voter_min_proficiency, nomination_start, nomination_end, voting_end, finalized_at) VALUES
    ('election_001', 'dao_cs', 'Q1 2026 CS Committee Election', 'Annual election for the Computer Science DAO committee seats', 'finalized', 5, 'apply', 'remember', '2025-12-01T00:00:00', '2025-12-15T00:00:00', '2025-12-31T00:00:00', '2026-01-02T00:00:00'),
    ('election_002', 'dao_web', 'Q2 2026 Web Dev Committee Election', 'Election for Web Development DAO committee seats', 'voting', 5, 'apply', 'remember', '2026-03-01T00:00:00', '2026-03-15T00:00:00', '2026-04-15T00:00:00', NULL),
    ('election_003', 'dao_design', 'Q2 2026 Design Committee Election', 'Election for Design DAO committee seats', 'nomination', 5, 'apply', 'remember', '2026-04-01T00:00:00', '2026-04-30T00:00:00', NULL, NULL);

-- Election nominees
INSERT INTO governance_election_nominees (id, election_id, stake_address, accepted, votes_received, is_winner) VALUES
    -- Finalized CS election: 4 nominees, 3 won
    ('nom_001', 'election_001', 'addr_seed_author_1',  1, 12, 1),
    ('nom_002', 'election_001', 'addr_seed_author_2',  1, 9,  1),
    ('nom_003', 'election_001', 'addr_seed_member_1',  1, 8,  1),
    ('nom_004', 'election_001', 'addr_seed_member_2',  1, 4,  0),
    -- Active Web election: 3 nominees, voting in progress
    ('nom_005', 'election_002', 'addr_seed_author_1',  1, 6, 0),
    ('nom_006', 'election_002', 'addr_seed_member_2',  1, 4, 0),
    ('nom_007', 'election_002', 'addr_seed_member_9',  1, 3, 0),
    -- Design nomination: 2 nominees so far
    ('nom_008', 'election_003', 'addr_seed_author_3',  1, 0, 0),
    ('nom_009', 'election_003', 'addr_seed_member_12', 0, 0, 0);

-- Election votes (for finalized + active elections)
INSERT INTO governance_election_votes (id, election_id, voter, nominee_id) VALUES
    ('evote_001', 'election_001', 'addr_seed_member_3', 'nom_001'),
    ('evote_002', 'election_001', 'addr_seed_member_4', 'nom_001'),
    ('evote_003', 'election_001', 'addr_seed_member_5', 'nom_002'),
    ('evote_004', 'election_001', 'addr_seed_member_6', 'nom_003'),
    ('evote_005', 'election_002', 'addr_seed_member_10', 'nom_005'),
    ('evote_006', 'election_002', 'addr_seed_member_11', 'nom_005'),
    ('evote_007', 'election_002', 'addr_seed_member_3',  'nom_006');

-- Proposals: varied states across DAOs
INSERT INTO governance_proposals (id, dao_id, title, description, category, status, proposer, votes_for, votes_against, voting_deadline, min_vote_proficiency) VALUES
    ('prop_001', 'dao_cs', 'Add Quantum Computing subject', 'Proposal to add Quantum Computing as a new subject under Computer Science, with skills for quantum gates, Shor/Grover algorithms, and quantum error correction.', 'taxonomy_change', 'approved', 'addr_seed_author_1', 5, 1, '2026-02-28T00:00:00', 'apply'),
    ('prop_002', 'dao_cs', 'Require 3 evidence records for analyze-level proofs', 'Increase minimum evidence threshold for analyze-level skill proofs from 2 to 3 to improve credential rigor.', 'policy', 'active', 'addr_seed_member_1', 3, 2, '2026-04-30T00:00:00', 'remember'),
    ('prop_003', 'dao_web', 'Add WebAssembly skill under Frontend', 'Proposal to add WASM as a new skill under Frontend Development: compiling Rust/C++ to WebAssembly, JS interop, and performance optimization.', 'taxonomy_change', 'active', 'addr_seed_author_1', 2, 0, '2026-04-20T00:00:00', 'apply'),
    ('prop_004', 'dao_design', 'Content moderation policy for design courses', 'Establish guidelines for reviewing design course content: original work requirements, attribution standards, and accessibility compliance.', 'content_moderation', 'draft', 'addr_seed_author_3', 0, 0, NULL, 'remember'),
    ('prop_005', 'dao_math', 'Add Applied Mathematics subject', 'Create a new Applied Mathematics subject covering numerical methods, optimization, and mathematical modelling.', 'taxonomy_change', 'rejected', 'addr_seed_member_7', 1, 4, '2026-03-15T00:00:00', 'apply');

-- Proposal votes
INSERT INTO governance_proposal_votes (id, proposal_id, voter, in_favor) VALUES
    ('pvote_001', 'prop_001', 'addr_seed_author_2',  1),
    ('pvote_002', 'prop_001', 'addr_seed_member_1',  1),
    ('pvote_003', 'prop_001', 'addr_seed_member_2',  1),
    ('pvote_004', 'prop_001', 'addr_seed_member_3',  1),
    ('pvote_005', 'prop_001', 'addr_seed_member_4',  1),
    ('pvote_006', 'prop_001', 'addr_seed_member_5',  0),
    ('pvote_007', 'prop_002', 'addr_seed_author_1',  1),
    ('pvote_008', 'prop_002', 'addr_seed_author_2',  1),
    ('pvote_009', 'prop_002', 'addr_seed_member_1',  1),
    ('pvote_010', 'prop_002', 'addr_seed_member_3',  0),
    ('pvote_011', 'prop_002', 'addr_seed_member_4',  0),
    ('pvote_012', 'prop_003', 'addr_seed_member_2',  1),
    ('pvote_013', 'prop_003', 'addr_seed_member_9',  1),
    ('pvote_014', 'prop_005', 'addr_seed_author_2',  0),
    ('pvote_015', 'prop_005', 'addr_seed_member_1',  0),
    ('pvote_016', 'prop_005', 'addr_seed_member_6',  0),
    ('pvote_017', 'prop_005', 'addr_seed_member_8',  0),
    ('pvote_018', 'prop_005', 'addr_seed_member_7',  1);

-- ============================================================
-- P5: CLASSROOMS
-- ============================================================
INSERT INTO classrooms (id, name, description, owner_address) VALUES
    ('class_algo_study', 'Algorithms Study Group', 'A collaborative space for learners working through Algorithms 101. Share solutions, discuss approaches, and prep for assessments.', 'addr_seed_author_1'),
    ('class_web_cohort', 'Web Dev Cohort — Spring 2026', 'Spring 2026 cohort for the Full-Stack Web Development course. Weekly sync calls, code reviews, and project feedback.', 'addr_seed_author_1'),
    ('class_design_crit', 'Design Critique Circle', 'Weekly design critiques and portfolio reviews. Share your work, get constructive feedback, and improve together.', 'addr_seed_author_3');

INSERT INTO classroom_members (classroom_id, stake_address, role, joined_at) VALUES
    -- Algo study group
    ('class_algo_study', 'addr_seed_author_1',   'owner',     '2026-01-20T10:00:00'),
    ('class_algo_study', 'addr_seed_learner_1',  'member',    '2026-01-21T09:30:00'),
    ('class_algo_study', 'addr_seed_learner_2',  'member',    '2026-01-22T14:00:00'),
    ('class_algo_study', 'addr_seed_learner_3',  'member',    '2026-01-23T11:15:00'),
    ('class_algo_study', 'addr_seed_learner_4',  'member',    '2026-01-25T16:00:00'),
    -- Web cohort
    ('class_web_cohort', 'addr_seed_author_1',   'owner',     '2026-02-01T09:00:00'),
    ('class_web_cohort', 'addr_seed_learner_1',  'member',    '2026-02-02T10:00:00'),
    ('class_web_cohort', 'addr_seed_learner_5',  'member',    '2026-02-03T11:30:00'),
    ('class_web_cohort', 'addr_seed_member_10',  'member',    '2026-02-04T14:00:00'),
    -- Design crit circle
    ('class_design_crit', 'addr_seed_author_3',  'owner',     '2026-03-01T10:00:00'),
    ('class_design_crit', 'addr_seed_learner_1', 'member',    '2026-03-02T09:00:00'),
    ('class_design_crit', 'addr_seed_member_13', 'member',    '2026-03-03T13:30:00');

INSERT INTO classroom_channels (id, classroom_id, name, description, channel_type) VALUES
    ('chan_001', 'class_algo_study', 'general',     'General discussion and announcements', 'text'),
    ('chan_002', 'class_algo_study', 'help',        'Ask for help with problems and concepts', 'text'),
    ('chan_003', 'class_web_cohort', 'general',     'Cohort announcements and weekly updates', 'text'),
    ('chan_004', 'class_web_cohort', 'code-review', 'Share code for peer review', 'text'),
    ('chan_005', 'class_web_cohort', 'standups',    'Async daily standups — what did you learn today?', 'text'),
    ('chan_006', 'class_design_crit', 'general',    'Announcements and scheduling', 'text'),
    ('chan_007', 'class_design_crit', 'critique',   'Post your designs for feedback', 'text');

INSERT INTO classroom_messages (id, channel_id, classroom_id, sender_address, content, sent_at) VALUES
    ('msg_001', 'chan_001', 'class_algo_study', 'addr_seed_author_1',  'Welcome to the Algorithms Study Group! Post questions anytime, and lets use the #help channel for specific problem discussions.', '2026-01-20T10:05:00'),
    ('msg_002', 'chan_001', 'class_algo_study', 'addr_seed_learner_1', 'Thanks for setting this up! Im working through chapter 2 on linked lists — anyone else at that point?', '2026-01-21T09:45:00'),
    ('msg_003', 'chan_001', 'class_algo_study', 'addr_seed_learner_3', 'Just finished the arrays quiz with a 95%. The amortized analysis question was tricky.', '2026-01-23T14:30:00'),
    ('msg_004', 'chan_002', 'class_algo_study', 'addr_seed_learner_2', 'Can someone explain why the time complexity of building a heap is O(n) and not O(n log n)? The sift-down approach is confusing me.', '2026-01-24T10:00:00'),
    ('msg_005', 'chan_002', 'class_algo_study', 'addr_seed_author_1',  'Great question! The key insight is that most nodes are near the bottom of the heap, so their sift-down cost is O(1). The mathematical proof uses the fact that the sum of h/2^h converges to 2.', '2026-01-24T10:30:00'),
    ('msg_006', 'chan_002', 'class_algo_study', 'addr_seed_learner_2', 'That makes so much more sense now — thanks!', '2026-01-24T10:45:00'),
    ('msg_007', 'chan_003', 'class_web_cohort', 'addr_seed_author_1',  'Welcome to the Spring 2026 Web Dev Cohort! Well have weekly sync calls on Thursdays at 6pm UTC. First call this Thursday.', '2026-02-01T09:15:00'),
    ('msg_008', 'chan_004', 'class_web_cohort', 'addr_seed_learner_5', 'Just pushed my first Vue component — a todo list with Composition API. Would love feedback on the reactivity patterns.', '2026-02-10T15:00:00'),
    ('msg_009', 'chan_004', 'class_web_cohort', 'addr_seed_author_1',  'Nice work! One suggestion: use computed() instead of watch() for derived state. Its more declarative and Vue can optimize it better.', '2026-02-10T16:30:00'),
    ('msg_010', 'chan_005', 'class_web_cohort', 'addr_seed_learner_1', 'Today: finished the REST API chapter. Finally understand why PUT is idempotent but POST isnt.', '2026-02-20T18:00:00'),
    ('msg_011', 'chan_006', 'class_design_crit', 'addr_seed_author_3',  'Welcome to the Design Critique Circle! Post your work in #critique anytime, and well do live critique sessions every Friday at 3pm UTC.', '2026-03-01T10:15:00'),
    ('msg_012', 'chan_007', 'class_design_crit', 'addr_seed_learner_1', 'Sharing my first wireframe for a learning dashboard. Looking for feedback on the information hierarchy — is the skill progress too buried?', '2026-03-05T14:00:00'),
    ('msg_013', 'chan_007', 'class_design_crit', 'addr_seed_author_3',  'Good start! I would move the skill progress above the course list — its the primary metric learners care about. Also consider a sparkline showing progress over time.', '2026-03-05T15:30:00'),
    ('msg_014', 'chan_007', 'class_design_crit', 'addr_seed_member_13', 'Agree with the feedback above. Also the color contrast on the secondary text might not meet WCAG AA — try bumping it to at least 4.5:1.', '2026-03-05T16:00:00');

-- Join request for the approval-required classroom
INSERT INTO classroom_join_requests (id, classroom_id, stake_address, message, status) VALUES
    ('jr_001', 'class_web_cohort', 'addr_seed_learner_6', 'Hi, I am enrolled in the Web Dev course and would love to join the cohort for code reviews and weekly syncs.', 'pending'),
    ('jr_002', 'class_web_cohort', 'addr_seed_learner_7', 'Currently in chapter 3 of the course. Looking for study partners!', 'approved');

-- ============================================================
-- P6: SENTINEL (integrity), TUTORING, APP SETTINGS
-- ============================================================
-- Integrity sessions (tied to algo enrollment)
INSERT INTO integrity_sessions (id, enrollment_id, status, integrity_score, started_at, ended_at) VALUES
    ('isess_001', 'enroll_algo', 'completed', 0.94, '2026-01-20T10:00:00', '2026-01-20T10:45:00'),
    ('isess_002', 'enroll_algo', 'completed', 0.91, '2026-02-08T10:00:00', '2026-02-08T11:00:00'),
    ('isess_003', 'enroll_web',  'completed', 0.96, '2026-02-06T09:30:00', '2026-02-06T10:00:00'),
    ('isess_004', 'enroll_ml',   'completed', 0.89, '2026-03-16T10:45:00', '2026-03-16T11:30:00');

-- Integrity snapshots (behavioral signals per session)
INSERT INTO integrity_snapshots (id, session_id, typing_score, mouse_score, human_score, tab_score, paste_score, devtools_score, camera_score, composite_score, captured_at) VALUES
    ('isnap_001a', 'isess_001', 0.95, 0.92, 0.98, 1.0, 1.0, 1.0, 0.88, 0.94, '2026-01-20T10:05:00'),
    ('isnap_001b', 'isess_001', 0.93, 0.94, 0.97, 1.0, 1.0, 1.0, 0.90, 0.95, '2026-01-20T10:15:00'),
    ('isnap_001c', 'isess_001', 0.96, 0.91, 0.96, 1.0, 1.0, 1.0, 0.87, 0.93, '2026-01-20T10:30:00'),
    ('isnap_002a', 'isess_002', 0.91, 0.88, 0.95, 1.0, 0.95, 1.0, 0.85, 0.91, '2026-02-08T10:10:00'),
    ('isnap_002b', 'isess_002', 0.89, 0.90, 0.94, 1.0, 1.0,  1.0, 0.86, 0.92, '2026-02-08T10:30:00'),
    ('isnap_002c', 'isess_002', 0.92, 0.87, 0.96, 0.95, 1.0, 1.0, 0.88, 0.91, '2026-02-08T10:50:00'),
    ('isnap_003a', 'isess_003', 0.97, 0.95, 0.99, 1.0, 1.0, 1.0, 0.92, 0.96, '2026-02-06T09:40:00'),
    ('isnap_003b', 'isess_003', 0.96, 0.96, 0.98, 1.0, 1.0, 1.0, 0.94, 0.97, '2026-02-06T09:55:00'),
    ('isnap_004a', 'isess_004', 0.88, 0.85, 0.92, 1.0, 1.0, 1.0, 0.80, 0.89, '2026-03-16T11:00:00'),
    ('isnap_004b', 'isess_004', 0.90, 0.84, 0.91, 0.90, 1.0, 1.0, 0.82, 0.88, '2026-03-16T11:15:00');

-- Tutoring sessions
INSERT INTO tutoring_sessions (id, title, status, created_at, ended_at) VALUES
    ('tutor_001', 'Dynamic Programming — Top-down vs Bottom-up', 'ended', '2026-02-25T15:05:00', '2026-02-25T16:00:00'),
    ('tutor_002', 'Wireframing Review — Learning Dashboard', 'ended', '2026-03-10T14:02:00', '2026-03-10T15:00:00'),
    ('tutor_003', 'Graph Algorithms — BFS & DFS Walkthrough', 'active', '2026-04-10T16:00:00', NULL);

-- App settings
INSERT INTO app_settings (key, value) VALUES
    ('theme', 'dark'),
    ('language', 'en'),
    ('notifications_enabled', 'true'),
    ('auto_sync', 'true'),
    ('sentinel_camera_enabled', 'true'),
    ('sentinel_keyboard_enabled', 'true');

-- Re-enable FK checks
PRAGMA foreign_keys = ON;

"##;

// Gate tests behind `has_app_lib` because this file is shared with the CLI
// crate via `#[path]`, and the tests depend on `crate::db::Database` which
// only exists in the main Tauri crate.
#[cfg(all(test, has_app_lib))]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn seed_inserts_data() {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");

        let inserted = seed_if_empty(db.conn()).expect("seed");
        assert!(inserted, "should insert seed data on empty db");

        // Verify counts
        let fields: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM subject_fields", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fields, 6);

        let subjects: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM subjects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(subjects, 18);

        let skills: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM skills", [], |r| r.get(0))
            .unwrap();
        assert!(skills >= 70, "expected >= 70 skills, got {}", skills);

        let prereqs: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM skill_prerequisites", [], |r| r.get(0))
            .unwrap();
        assert!(
            prereqs >= 50,
            "expected >= 50 prerequisite edges, got {}",
            prereqs
        );

        let relations: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM skill_relations", [], |r| r.get(0))
            .unwrap();
        assert!(
            relations >= 10,
            "expected >= 10 relation edges, got {}",
            relations
        );

        let daos: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM governance_daos", [], |r| r.get(0))
            .unwrap();
        assert_eq!(daos, 6);

        let courses: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM courses", [], |r| r.get(0))
            .unwrap();
        assert_eq!(courses, 6);

        let chapters: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM course_chapters", [], |r| r.get(0))
            .unwrap();
        assert!(chapters >= 20, "expected >= 20 chapters, got {}", chapters);

        let elements: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM course_elements", [], |r| r.get(0))
            .unwrap();
        assert!(elements >= 80, "expected >= 80 elements, got {}", elements);

        // Verify fair representation of element types
        let element_types: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(DISTINCT element_type) FROM course_elements",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            element_types >= 9,
            "expected >= 9 distinct element types, got {}",
            element_types
        );

        let tags: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM element_skill_tags", [], |r| r.get(0))
            .unwrap();
        assert!(
            tags >= 90,
            "expected >= 90 element-skill tags, got {}",
            tags
        );
    }

    #[test]
    fn seed_populates_visual_assets() {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");
        seed_if_empty(db.conn()).expect("seed");

        // Check thumbnail_svg
        let svg_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM courses WHERE thumbnail_svg IS NOT NULL AND thumbnail_svg != ''",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(svg_count, 6, "all 6 courses should have thumbnail_svg");

        // Check author_name
        let author_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM courses WHERE author_name IS NOT NULL AND author_name != ''",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(author_count, 6, "all 6 courses should have author_name");

        // Check a specific SVG starts correctly
        let svg: String = db
            .conn()
            .query_row(
                "SELECT thumbnail_svg FROM courses WHERE id = 'course_algo_101'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            svg.starts_with("<svg"),
            "SVG should start with <svg, got: {}",
            &svg[..40.min(svg.len())]
        );
    }

    #[test]
    fn seed_is_idempotent() {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");

        let first = seed_if_empty(db.conn()).expect("first seed");
        assert!(first);

        let second = seed_if_empty(db.conn()).expect("second seed");
        assert!(!second, "should skip seed on non-empty db");

        // Counts unchanged
        let fields: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM subject_fields", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fields, 6);
    }
}
