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
        return Ok(false);
    }

    log::info!("Seeding database with demo taxonomy, courses, and governance data…");

    conn.execute_batch(SEED_SQL)?;

    // Visual assets are applied via parameterized queries (not execute_batch)
    // because sqlite3_exec can silently fail on long SVG strings or emoji.
    seed_visual_assets(conn)?;

    log::info!("Seed data inserted successfully");
    Ok(true)
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

-- ============================================================
-- ELEMENTS (a few per chapter to demonstrate the player)
-- ============================================================
INSERT INTO course_elements (id, chapter_id, title, element_type, position, duration_seconds) VALUES
    -- Algo course - Chapter 1
    ('el_algo_1_1', 'ch_algo_1', 'What is Big-O?',                       'text',  0, NULL),
    ('el_algo_1_2', 'ch_algo_1', 'Analyzing Loops',                      'text',  1, NULL),
    ('el_algo_1_3', 'ch_algo_1', 'Complexity Quiz',                      'quiz',  2, NULL),
    -- Algo course - Chapter 2
    ('el_algo_2_1', 'ch_algo_2', 'Array Operations',                     'text',  0, NULL),
    ('el_algo_2_2', 'ch_algo_2', 'Linked List Implementation',           'text',  1, NULL),
    ('el_algo_2_3', 'ch_algo_2', 'Stack & Queue Patterns',               'text',  2, NULL),
    ('el_algo_2_4', 'ch_algo_2', 'Data Structures Quiz',                 'quiz',  3, NULL),
    -- Algo course - Chapter 3
    ('el_algo_3_1', 'ch_algo_3', 'Binary Trees Explained',               'text',  0, NULL),
    ('el_algo_3_2', 'ch_algo_3', 'Graph Representations',                'text',  1, NULL),
    ('el_algo_3_3', 'ch_algo_3', 'BFS vs DFS',                           'text',  2, NULL),
    ('el_algo_3_4', 'ch_algo_3', 'Trees & Graphs Quiz',                  'quiz',  3, NULL),
    -- Algo course - Chapter 4
    ('el_algo_4_1', 'ch_algo_4', 'Bubble Sort & Selection Sort',         'text',  0, NULL),
    ('el_algo_4_2', 'ch_algo_4', 'Merge Sort & Quick Sort',              'text',  1, NULL),
    ('el_algo_4_3', 'ch_algo_4', 'Sorting Quiz',                         'quiz',  2, NULL),
    -- Algo course - Chapter 5
    ('el_algo_5_1', 'ch_algo_5', 'Hash Functions',                       'text',  0, NULL),
    ('el_algo_5_2', 'ch_algo_5', 'Collision Resolution',                 'text',  1, NULL),
    ('el_algo_5_3', 'ch_algo_5', 'Hash Tables Quiz',                     'quiz',  2, NULL),

    -- Web course - selected elements
    ('el_web_1_1',  'ch_web_1', 'Semantic HTML',                         'text',  0, NULL),
    ('el_web_1_2',  'ch_web_1', 'Flexbox & Grid',                        'text',  1, NULL),
    ('el_web_2_1',  'ch_web_2', 'ES6+ Features',                         'text',  0, NULL),
    ('el_web_2_2',  'ch_web_2', 'Async/Await Patterns',                  'text',  1, NULL),
    ('el_web_3_1',  'ch_web_3', 'TypeScript Type System',                'text',  0, NULL),
    ('el_web_4_1',  'ch_web_4', 'Vue Reactivity System',                 'text',  0, NULL),
    ('el_web_4_2',  'ch_web_4', 'Composables Pattern',                   'text',  1, NULL),
    ('el_web_5_1',  'ch_web_5', 'Building REST APIs in Rust',            'text',  0, NULL),
    ('el_web_5_2',  'ch_web_5', 'Database Design & Migrations',          'text',  1, NULL),
    ('el_web_5_3',  'ch_web_5', 'Authentication with JWT',               'text',  2, NULL),

    -- ML course - selected elements
    ('el_ml_1_1',   'ch_ml_1', 'Linear Regression from Scratch',         'text',  0, NULL),
    ('el_ml_1_2',   'ch_ml_1', 'Logistic Regression',                    'text',  1, NULL),
    ('el_ml_2_1',   'ch_ml_2', 'Decision Trees & Random Forests',        'text',  0, NULL),
    ('el_ml_2_2',   'ch_ml_2', 'K-Means Clustering',                     'text',  1, NULL),
    ('el_ml_3_1',   'ch_ml_3', 'Neural Network Architecture',            'text',  0, NULL),
    ('el_ml_3_2',   'ch_ml_3', 'Backpropagation',                        'text',  1, NULL),
    ('el_ml_4_1',   'ch_ml_4', 'Cross-Validation Techniques',            'text',  0, NULL),
    ('el_ml_4_2',   'ch_ml_4', 'Evaluation Metrics Quiz',                'quiz',  1, NULL),

    -- Crypto course - selected elements
    ('el_cry_1_1',  'ch_cry_1', 'Block Ciphers & AES',                   'text',  0, NULL),
    ('el_cry_2_1',  'ch_cry_2', 'RSA Explained',                         'text',  0, NULL),
    ('el_cry_2_2',  'ch_cry_2', 'Elliptic Curve Cryptography',           'text',  1, NULL),
    ('el_cry_3_1',  'ch_cry_3', 'SHA-256 & BLAKE2',                      'text',  0, NULL),
    ('el_cry_3_2',  'ch_cry_3', 'Digital Signatures with Ed25519',       'text',  1, NULL),
    ('el_cry_4_1',  'ch_cry_4', 'Introduction to ZK Proofs',             'text',  0, NULL),

    -- UX course - selected elements
    ('el_ux_1_1',   'ch_ux_1', 'Planning User Interviews',               'text',  0, NULL),
    ('el_ux_1_2',   'ch_ux_1', 'Creating Personas',                      'text',  1, NULL),
    ('el_ux_2_1',   'ch_ux_2', 'Card Sorting Workshop',                  'text',  0, NULL),
    ('el_ux_2_2',   'ch_ux_2', 'Navigation Patterns',                    'text',  1, NULL),
    ('el_ux_3_1',   'ch_ux_3', 'Low-Fidelity Wireframes',                'text',  0, NULL),
    ('el_ux_3_2',   'ch_ux_3', 'Interactive Prototyping',                 'text',  1, NULL),
    ('el_ux_4_1',   'ch_ux_4', 'Color Theory for Screens',               'text',  0, NULL),
    ('el_ux_4_2',   'ch_ux_4', 'Typography Best Practices',              'text',  1, NULL);

-- ============================================================
-- ELEMENT SKILL TAGS (link elements to skills for evidence)
-- ============================================================
INSERT INTO element_skill_tags (element_id, skill_id, weight) VALUES
    -- Algo course
    ('el_algo_1_1', 'skill_big_o',          1.0),
    ('el_algo_1_2', 'skill_big_o',          1.0),
    ('el_algo_1_3', 'skill_big_o',          1.0),
    ('el_algo_2_1', 'skill_arrays',         1.0),
    ('el_algo_2_2', 'skill_linked_lists',   1.0),
    ('el_algo_2_3', 'skill_stacks_queues',  1.0),
    ('el_algo_2_4', 'skill_arrays',         0.5),
    ('el_algo_2_4', 'skill_linked_lists',   0.5),
    ('el_algo_2_4', 'skill_stacks_queues',  0.5),
    ('el_algo_3_1', 'skill_trees',          1.0),
    ('el_algo_3_2', 'skill_graphs',         1.0),
    ('el_algo_3_3', 'skill_graphs',         1.0),
    ('el_algo_3_4', 'skill_trees',          0.5),
    ('el_algo_3_4', 'skill_graphs',         0.5),
    ('el_algo_4_1', 'skill_sorting',        1.0),
    ('el_algo_4_2', 'skill_sorting',        1.0),
    ('el_algo_4_3', 'skill_sorting',        1.0),
    ('el_algo_5_1', 'skill_hashing',        1.0),
    ('el_algo_5_2', 'skill_hashing',        1.0),
    ('el_algo_5_3', 'skill_hashing',        1.0),

    -- Web course
    ('el_web_1_1',  'skill_html_css',       1.0),
    ('el_web_1_2',  'skill_html_css',       1.0),
    ('el_web_2_1',  'skill_javascript',     1.0),
    ('el_web_2_2',  'skill_javascript',     1.0),
    ('el_web_3_1',  'skill_typescript',     1.0),
    ('el_web_4_1',  'skill_vue',            1.0),
    ('el_web_4_2',  'skill_vue',            1.0),
    ('el_web_5_1',  'skill_rest_api',       1.0),
    ('el_web_5_1',  'skill_rust',           0.5),
    ('el_web_5_2',  'skill_db_design',      1.0),
    ('el_web_5_3',  'skill_auth',           1.0),

    -- ML course
    ('el_ml_1_1',   'skill_regression',     1.0),
    ('el_ml_1_2',   'skill_regression',     1.0),
    ('el_ml_2_1',   'skill_supervised',     1.0),
    ('el_ml_2_2',   'skill_unsupervised',   1.0),
    ('el_ml_3_1',   'skill_neural_nets',    1.0),
    ('el_ml_3_2',   'skill_neural_nets',    1.0),
    ('el_ml_4_1',   'skill_ml_eval',        1.0),
    ('el_ml_4_2',   'skill_ml_eval',        1.0),

    -- Crypto course
    ('el_cry_1_1',  'skill_symmetric',      1.0),
    ('el_cry_2_1',  'skill_asymmetric',     1.0),
    ('el_cry_2_2',  'skill_asymmetric',     1.0),
    ('el_cry_3_1',  'skill_hash_crypto',    1.0),
    ('el_cry_3_2',  'skill_signatures',     1.0),
    ('el_cry_4_1',  'skill_zk',            1.0),

    -- UX course
    ('el_ux_1_1',   'skill_user_research',  1.0),
    ('el_ux_1_2',   'skill_user_research',  1.0),
    ('el_ux_2_1',   'skill_ia',            1.0),
    ('el_ux_2_2',   'skill_ia',            1.0),
    ('el_ux_3_1',   'skill_wireframing',    1.0),
    ('el_ux_3_2',   'skill_wireframing',    1.0),
    ('el_ux_4_1',   'skill_color_theory',   1.0),
    ('el_ux_4_2',   'skill_typography',     1.0);

"##;

#[cfg(test)]
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
        assert_eq!(courses, 5);

        let chapters: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM course_chapters", [], |r| r.get(0))
            .unwrap();
        assert!(chapters >= 20, "expected >= 20 chapters, got {}", chapters);

        let elements: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM course_elements", [], |r| r.get(0))
            .unwrap();
        assert!(elements >= 40, "expected >= 40 elements, got {}", elements);

        let tags: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM element_skill_tags", [], |r| r.get(0))
            .unwrap();
        assert!(
            tags >= 40,
            "expected >= 40 element-skill tags, got {}",
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
        assert_eq!(svg_count, 5, "all 5 courses should have thumbnail_svg");

        // Check author_name
        let author_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM courses WHERE author_name IS NOT NULL AND author_name != ''",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(author_count, 5, "all 5 courses should have author_name");

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
