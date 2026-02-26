//! Seed content for dev/testnet course elements.
//!
//! Contains inline content for all seed elements (HTML for text,
//! JSON for quizzes/MCQs/essays). Also provides
//! `seed_content_if_needed()` which writes content into iroh blobs
//! and populates `content_cid`.

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::db::Database;
use crate::ipfs::content;
use crate::ipfs::node::ContentNode;

/// Seed content into iroh for elements that lack a `content_cid`.
/// Returns the number of elements updated, or 0 if skipped.
pub async fn seed_content_if_needed(
    db: &Arc<Mutex<Database>>,
    node: &Arc<ContentNode>,
) -> Result<u32, String> {
    // Check if any seed element already has content — if so, skip entirely.
    let needs_seed = {
        let db = db.lock().await;
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM course_elements WHERE id LIKE 'el_%' AND content_cid IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        count == 0
    };

    if !needs_seed {
        log::info!("Seed elements already have content — skipping content seed");
        return Ok(0);
    }

    log::info!("Seeding content blobs for dev/testnet elements…");

    // Phase 1: Add all blobs to iroh WITHOUT holding the DB lock.
    // This is the slow part and must not block other DB consumers.
    let mut pending: Vec<(&str, String)> = Vec::new();
    for (element_id, body) in SEED_CONTENT {
        let result = content::add_bytes(node, body.as_bytes())
            .await
            .map_err(|e| format!("failed to add content for {element_id}: {e}"))?;
        pending.push((element_id, result.hash.clone()));
    }

    // Phase 2: Single DB write lock — batch-update all rows in a transaction.
    let updated = {
        let db = db.lock().await;
        let conn = db.conn();
        conn.execute_batch("BEGIN")
            .map_err(|e| format!("begin tx: {e}"))?;

        let mut count = 0u32;
        for (element_id, hash) in &pending {
            conn.execute(
                "UPDATE course_elements SET content_cid = ?1 WHERE id = ?2",
                rusqlite::params![hash, element_id],
            )
            .map_err(|e| format!("failed to update {element_id}: {e}"))?;
            count += 1;
        }

        conn.execute_batch("COMMIT")
            .map_err(|e| format!("commit tx: {e}"))?;
        count
    };

    log::info!("Seeded content for {updated} elements");
    Ok(updated)
}

// ---------------------------------------------------------------------------
// Inline content for each seed element.
//
// Text elements: minimal HTML (rendered via v-html in TextContent.vue).
// Quiz elements: QuizDefinition JSON (parsed by QuizEngine.vue).
//
// This is dev/testnet data — short and functional, not a textbook.
// ---------------------------------------------------------------------------

pub const SEED_CONTENT: &[(&str, &str)] = &[
    // ======================================================================
    // ALGORITHMS COURSE
    // ======================================================================

    // Chapter 1: Complexity Analysis
    ("el_algo_1_1", r#"<h2>What is Big-O?</h2>
<p>Big-O notation describes the upper bound of an algorithm's growth rate as input size increases. We write <code>O(f(n))</code> to say the runtime grows no faster than <code>f(n)</code> for large <code>n</code>.</p>
<p>Common complexity classes, from fastest to slowest:</p>
<ul>
  <li><strong>O(1)</strong> — constant (hash table lookup)</li>
  <li><strong>O(log n)</strong> — logarithmic (binary search)</li>
  <li><strong>O(n)</strong> — linear (scanning an array)</li>
  <li><strong>O(n log n)</strong> — linearithmic (merge sort)</li>
  <li><strong>O(n²)</strong> — quadratic (bubble sort)</li>
  <li><strong>O(2ⁿ)</strong> — exponential (brute-force subset enumeration)</li>
</ul>
<p>When analyzing complexity, we drop constants and lower-order terms: <code>3n² + 5n + 2</code> simplifies to <code>O(n²)</code>.</p>"#),

    ("el_algo_1_2", r#"<h2>Analyzing Loops</h2>
<p>The most common source of complexity is loops. A single loop over <code>n</code> items is <code>O(n)</code>. Nested loops multiply:</p>
<pre><code>for i in 0..n {       // O(n)
    for j in 0..n {   // × O(n)
        // O(1) work
    }
}
// Total: O(n²)</code></pre>
<p>A loop that halves the problem each iteration (like binary search) is <code>O(log n)</code>:</p>
<pre><code>while lo < hi {
    let mid = (lo + hi) / 2;
    if arr[mid] < target { lo = mid + 1; }
    else { hi = mid; }
}</code></pre>
<p>Recursive algorithms use the <strong>Master Theorem</strong> or recurrence relations. For example, merge sort splits in half and does linear work per level: <code>T(n) = 2T(n/2) + O(n) = O(n log n)</code>.</p>"#),

    ("el_algo_1_3", QUIZ_COMPLEXITY),

    // Chapter 2: Linear Data Structures
    ("el_algo_2_1", r#"<h2>Array Operations</h2>
<p>Arrays store elements contiguously in memory, giving <code>O(1)</code> random access by index. Trade-offs:</p>
<table>
  <tr><th>Operation</th><th>Time</th></tr>
  <tr><td>Access by index</td><td>O(1)</td></tr>
  <tr><td>Search (unsorted)</td><td>O(n)</td></tr>
  <tr><td>Insert at end</td><td>O(1) amortized</td></tr>
  <tr><td>Insert at position</td><td>O(n)</td></tr>
  <tr><td>Delete at position</td><td>O(n)</td></tr>
</table>
<p>Dynamic arrays (like Rust's <code>Vec</code>) grow by doubling capacity when full, giving amortized O(1) push. The key insight: even though occasional resizes cost O(n), they happen so rarely that the average cost per operation stays constant.</p>"#),

    ("el_algo_2_2", r#"<h2>Linked List Implementation</h2>
<p>A linked list stores elements in nodes, each pointing to the next. Unlike arrays, insertion and deletion at a known position are <code>O(1)</code> — no shifting needed.</p>
<pre><code>struct Node&lt;T&gt; {
    value: T,
    next: Option&lt;Box&lt;Node&lt;T&gt;&gt;&gt;,
}</code></pre>
<p>Trade-offs vs arrays: no random access (<code>O(n)</code> to reach index <code>k</code>), more memory per element (pointer overhead), poor cache locality. Doubly-linked lists add a <code>prev</code> pointer for <code>O(1)</code> removal given a node reference.</p>
<p>In practice, arrays outperform linked lists for most workloads due to CPU cache effects. Linked lists shine when you need frequent insertion/removal in the middle and already hold a reference to the node.</p>"#),

    ("el_algo_2_3", r#"<h2>Stack &amp; Queue Patterns</h2>
<p><strong>Stack</strong> (LIFO): push and pop from the same end. Used for function call tracking, undo operations, expression parsing, and DFS.</p>
<pre><code>let mut stack = Vec::new();
stack.push(1);
stack.push(2);
assert_eq!(stack.pop(), Some(2)); // last in, first out</code></pre>
<p><strong>Queue</strong> (FIFO): enqueue at the back, dequeue from the front. Used for BFS, task scheduling, and buffering.</p>
<pre><code>use std::collections::VecDeque;
let mut queue = VecDeque::new();
queue.push_back(1);
queue.push_back(2);
assert_eq!(queue.pop_front(), Some(1)); // first in, first out</code></pre>
<p>Both are <code>O(1)</code> for their core operations when implemented correctly.</p>"#),

    ("el_algo_2_4", QUIZ_DATA_STRUCTURES),

    // Chapter 3: Trees & Graphs
    ("el_algo_3_1", r#"<h2>Binary Trees Explained</h2>
<p>A binary tree is a hierarchical structure where each node has at most two children (left and right). A <strong>Binary Search Tree</strong> (BST) adds an ordering invariant: left child &lt; parent &lt; right child.</p>
<p>BST operations — search, insert, delete — are <code>O(h)</code> where <code>h</code> is the tree height. A balanced BST has <code>h = O(log n)</code>, but a degenerate tree (all nodes in one direction) degrades to <code>O(n)</code>.</p>
<p>Tree traversals visit all nodes in a specific order:</p>
<ul>
  <li><strong>In-order</strong> (left → root → right): gives sorted output for BSTs</li>
  <li><strong>Pre-order</strong> (root → left → right): useful for serialization</li>
  <li><strong>Post-order</strong> (left → right → root): useful for deletion</li>
  <li><strong>Level-order</strong> (BFS): visits by depth level</li>
</ul>"#),

    ("el_algo_3_2", r#"<h2>Graph Representations</h2>
<p>A graph <code>G = (V, E)</code> consists of vertices and edges. Two standard representations:</p>
<p><strong>Adjacency Matrix</strong>: A 2D array where <code>matrix[i][j] = 1</code> if an edge exists. Space: <code>O(V²)</code>. Edge lookup: <code>O(1)</code>. Good for dense graphs.</p>
<p><strong>Adjacency List</strong>: Each vertex stores a list of its neighbors. Space: <code>O(V + E)</code>. Iterating neighbors: <code>O(degree)</code>. Good for sparse graphs (most real-world graphs).</p>
<pre><code>// Adjacency list in Rust
let mut adj: Vec&lt;Vec&lt;usize&gt;&gt; = vec![vec![]; n];
adj[0].push(1); // edge 0 → 1
adj[1].push(0); // edge 1 → 0 (undirected)</code></pre>
<p>Graphs can be directed or undirected, weighted or unweighted, cyclic or acyclic. A <strong>DAG</strong> (directed acyclic graph) is foundational for dependency resolution and topological sorting.</p>"#),

    ("el_algo_3_3", r#"<h2>BFS vs DFS</h2>
<p>Two fundamental graph traversal strategies:</p>
<p><strong>Breadth-First Search (BFS)</strong> explores all neighbors at the current depth before moving deeper. Uses a queue. Guarantees shortest path in unweighted graphs.</p>
<pre><code>fn bfs(adj: &[Vec&lt;usize&gt;], start: usize) {
    let mut visited = vec![false; adj.len()];
    let mut queue = VecDeque::from([start]);
    visited[start] = true;
    while let Some(u) = queue.pop_front() {
        for &v in &adj[u] {
            if !visited[v] {
                visited[v] = true;
                queue.push_back(v);
            }
        }
    }
}</code></pre>
<p><strong>Depth-First Search (DFS)</strong> explores as far as possible along a branch before backtracking. Uses a stack (or recursion). Useful for cycle detection, topological sort, and connected components.</p>
<p>Both are <code>O(V + E)</code> for adjacency lists.</p>"#),

    ("el_algo_3_4", QUIZ_TREES_GRAPHS),

    // Chapter 4: Sorting
    ("el_algo_4_1", r#"<h2>Bubble Sort &amp; Selection Sort</h2>
<p>These are simple <code>O(n²)</code> comparison sorts, useful for teaching but rarely used in production.</p>
<p><strong>Bubble Sort</strong> repeatedly swaps adjacent out-of-order elements. Each pass "bubbles" the largest unsorted element to its final position. Best case (already sorted): <code>O(n)</code> with early termination.</p>
<p><strong>Selection Sort</strong> finds the minimum element in the unsorted portion and swaps it to the front. Always <code>O(n²)</code> comparisons regardless of input, but does at most <code>O(n)</code> swaps — useful when writes are expensive.</p>
<p>Both are <strong>in-place</strong> (no extra memory beyond a few variables). Bubble sort is <strong>stable</strong> (preserves order of equal elements); selection sort is not.</p>"#),

    ("el_algo_4_2", r#"<h2>Merge Sort &amp; Quick Sort</h2>
<p><strong>Merge Sort</strong>: Divide the array in half, recursively sort each half, then merge. Always <code>O(n log n)</code>, but requires <code>O(n)</code> extra space for merging. Stable.</p>
<pre><code>fn merge_sort(arr: &amp;mut [i32]) {
    if arr.len() <= 1 { return; }
    let mid = arr.len() / 2;
    merge_sort(&amp;mut arr[..mid]);
    merge_sort(&amp;mut arr[mid..]);
    // merge the two sorted halves
}</code></pre>
<p><strong>Quick Sort</strong>: Pick a pivot, partition elements into "less than" and "greater than" groups, recurse on each. Average <code>O(n log n)</code>, worst case <code>O(n²)</code> with bad pivot choices. In-place. Not stable.</p>
<p>In practice, quick sort is often faster than merge sort due to better cache locality, despite the worse worst-case. Most standard library sort implementations use a hybrid (like introsort or pdqsort).</p>"#),

    ("el_algo_4_3", QUIZ_SORTING),

    // Chapter 5: Hash Tables
    ("el_algo_5_1", r#"<h2>Hash Functions</h2>
<p>A hash function maps keys to array indices: <code>index = hash(key) % capacity</code>. A good hash function is:</p>
<ul>
  <li><strong>Deterministic</strong>: same input always produces the same output</li>
  <li><strong>Uniform</strong>: distributes keys evenly across buckets</li>
  <li><strong>Fast</strong>: computable in O(1) time</li>
</ul>
<p>Rust's <code>HashMap</code> uses SipHash by default (resistant to hash-flooding DoS attacks). For cryptographic use, you'd use SHA-256 or BLAKE3 — much slower but collision-resistant.</p>
<p>The <strong>load factor</strong> α = n/m (items / capacity) determines performance. Most implementations resize when α exceeds ~0.75.</p>"#),

    ("el_algo_5_2", r#"<h2>Collision Resolution</h2>
<p>When two keys hash to the same index, we have a <strong>collision</strong>. Two main strategies:</p>
<p><strong>Chaining</strong>: Each bucket holds a linked list (or vec) of entries. Simple, degrades gracefully — worst case O(n) if all keys collide, but average O(1 + α).</p>
<p><strong>Open Addressing</strong>: Store all entries in the array itself. On collision, probe for the next empty slot:</p>
<ul>
  <li><em>Linear probing</em>: check index+1, index+2, … (cache-friendly but causes clustering)</li>
  <li><em>Quadratic probing</em>: check index+1², index+2², … (reduces clustering)</li>
  <li><em>Double hashing</em>: use a second hash function for step size</li>
</ul>
<p>Robin Hood hashing (used in Rust's <code>hashbrown</code>) is a variant of linear probing that moves existing entries to reduce variance in probe lengths.</p>"#),

    ("el_algo_5_3", QUIZ_HASH_TABLES),

    // ======================================================================
    // WEB DEVELOPMENT COURSE
    // ======================================================================

    ("el_web_1_1", r#"<h2>Semantic HTML</h2>
<p>Semantic elements describe their content's meaning, not just appearance. Using <code>&lt;article&gt;</code>, <code>&lt;nav&gt;</code>, <code>&lt;main&gt;</code>, <code>&lt;section&gt;</code>, <code>&lt;header&gt;</code>, and <code>&lt;footer&gt;</code> improves accessibility, SEO, and code readability.</p>
<p>Common anti-patterns: using <code>&lt;div&gt;</code> for everything, using <code>&lt;table&gt;</code> for layout, skipping heading levels. Screen readers rely on semantic structure to navigate the page.</p>
<p>Key rule: choose the element that best describes the content's <em>purpose</em>, not its visual appearance. Use CSS for styling.</p>"#),

    ("el_web_1_2", r#"<h2>Flexbox &amp; Grid</h2>
<p><strong>Flexbox</strong> is for one-dimensional layouts (row or column). Set <code>display: flex</code> on the container, then control alignment with <code>justify-content</code>, <code>align-items</code>, and <code>gap</code>.</p>
<pre><code>.container {
  display: flex;
  gap: 1rem;
  justify-content: space-between;
}</code></pre>
<p><strong>CSS Grid</strong> is for two-dimensional layouts. Define rows and columns explicitly:</p>
<pre><code>.grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 1rem;
}</code></pre>
<p>Rule of thumb: use Flexbox for component-level layout, Grid for page-level layout. They compose well together.</p>"#),

    ("el_web_2_1", r#"<h2>ES6+ Features</h2>
<p>Modern JavaScript (ES6/ES2015+) introduced features that are now standard:</p>
<ul>
  <li><code>let</code>/<code>const</code> — block-scoped variables (prefer <code>const</code>)</li>
  <li>Arrow functions: <code>const add = (a, b) =&gt; a + b</code></li>
  <li>Template literals: <code>`Hello ${name}`</code></li>
  <li>Destructuring: <code>const { x, y } = point</code></li>
  <li>Spread/rest: <code>[...arr]</code>, <code>{ ...obj }</code></li>
  <li>Modules: <code>import</code>/<code>export</code></li>
  <li>Optional chaining: <code>user?.address?.city</code></li>
  <li>Nullish coalescing: <code>value ?? 'default'</code></li>
</ul>
<p>These features make code more concise and expressive. TypeScript builds on top of ES6+ by adding a static type system.</p>"#),

    ("el_web_2_2", r#"<h2>Async/Await Patterns</h2>
<p>JavaScript is single-threaded but non-blocking, using an event loop for I/O. <code>async/await</code> provides clean syntax over Promises:</p>
<pre><code>async function fetchUser(id) {
  const res = await fetch(`/api/users/${id}`);
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return res.json();
}</code></pre>
<p>Key patterns:</p>
<ul>
  <li><strong>Parallel</strong>: <code>await Promise.all([fetchA(), fetchB()])</code></li>
  <li><strong>Sequential</strong>: <code>const a = await fetchA(); const b = await fetchB(a.id);</code></li>
  <li><strong>Error handling</strong>: wrap in try/catch, or use <code>.catch()</code></li>
</ul>
<p>Avoid: <code>await</code> inside loops when the iterations are independent (use <code>Promise.all</code> with <code>.map()</code> instead).</p>"#),

    ("el_web_3_1", r#"<h2>TypeScript Type System</h2>
<p>TypeScript adds compile-time type checking to JavaScript. Core concepts:</p>
<pre><code>// Interfaces define object shapes
interface User {
  id: string;
  name: string;
  email?: string; // optional
}

// Generics for reusable types
type Result&lt;T&gt; = { ok: true; data: T } | { ok: false; error: string };

// Union and literal types
type Status = 'active' | 'inactive' | 'pending';

// Type guards narrow types at runtime
function isUser(x: unknown): x is User {
  return typeof x === 'object' && x !== null && 'id' in x;
}</code></pre>
<p>TypeScript's type system is <em>structural</em> (duck typing), not nominal. Two types with the same shape are compatible regardless of name. Use <code>strict: true</code> in <code>tsconfig.json</code> for maximum safety.</p>"#),

    ("el_web_4_1", r#"<h2>Vue Reactivity System</h2>
<p>Vue 3's reactivity is built on JavaScript Proxies. When you create a <code>ref()</code> or <code>reactive()</code>, Vue wraps the value and tracks which components read it. When the value changes, only those components re-render.</p>
<pre><code>import { ref, computed, watch } from 'vue';

const count = ref(0);
const doubled = computed(() => count.value * 2);

watch(count, (newVal, oldVal) => {
  console.log(`changed from ${oldVal} to ${newVal}`);
});

count.value++; // triggers computed + watcher</code></pre>
<p><code>ref()</code> wraps primitives (access via <code>.value</code>). <code>reactive()</code> wraps objects (direct property access). In templates, <code>.value</code> is auto-unwrapped.</p>"#),

    ("el_web_4_2", r#"<h2>Composables Pattern</h2>
<p>Composables are functions that encapsulate reactive state and logic, following the naming convention <code>use*</code>. They're Vue's answer to React hooks:</p>
<pre><code>// composables/useCounter.ts
export function useCounter(initial = 0) {
  const count = ref(initial);
  const increment = () => count.value++;
  const decrement = () => count.value--;
  const reset = () => count.value = initial;
  return { count, increment, decrement, reset };
}</code></pre>
<p>Composables can use other composables, lifecycle hooks, and watchers. They promote code reuse without mixins or inheritance. Each call creates independent state.</p>
<p>Best practices: return <code>ref</code>s (not raw values) so consumers retain reactivity, keep composables focused on one concern, and handle cleanup in <code>onUnmounted</code>.</p>"#),

    ("el_web_5_1", r#"<h2>Building REST APIs in Rust</h2>
<p>Rust's type system and performance make it excellent for backend APIs. Common frameworks: Actix Web, Axum, Rocket. A minimal Axum handler:</p>
<pre><code>use axum::{Json, extract::Path};

async fn get_user(Path(id): Path&lt;String&gt;) -> Json&lt;User&gt; {
    let user = db::find_user(&id).await.unwrap();
    Json(user)
}</code></pre>
<p>Key Rust backend patterns:</p>
<ul>
  <li><strong>Extractors</strong>: parse request parts (path, query, body) into typed values</li>
  <li><strong>Middleware</strong>: logging, auth, CORS via tower layers</li>
  <li><strong>Error handling</strong>: implement <code>IntoResponse</code> for custom error types</li>
  <li><strong>Shared state</strong>: <code>Arc&lt;AppState&gt;</code> passed via extension</li>
</ul>"#),

    ("el_web_5_2", r#"<h2>Database Design &amp; Migrations</h2>
<p>Good schema design starts with identifying entities and relationships. Key principles:</p>
<ul>
  <li><strong>Normalization</strong>: eliminate redundancy (3NF is usually sufficient)</li>
  <li><strong>Foreign keys</strong>: enforce referential integrity</li>
  <li><strong>Indexes</strong>: add on columns used in WHERE, JOIN, and ORDER BY</li>
  <li><strong>Migrations</strong>: version-controlled schema changes (never edit production directly)</li>
</ul>
<pre><code>CREATE TABLE users (
    id    TEXT PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    name  TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_users_email ON users(email);</code></pre>
<p>For embedded apps, SQLite is ideal: zero configuration, single-file database, excellent read performance with WAL mode.</p>"#),

    ("el_web_5_3", r#"<h2>Authentication with JWT</h2>
<p>JSON Web Tokens (JWT) encode claims as a signed JSON payload. The flow:</p>
<ol>
  <li>User submits credentials (username + password)</li>
  <li>Server verifies, creates a JWT with user ID and expiry</li>
  <li>Client stores the token and sends it in <code>Authorization: Bearer &lt;token&gt;</code></li>
  <li>Server validates the signature on each request</li>
</ol>
<p>A JWT has three parts: <code>header.payload.signature</code> (base64url-encoded). The signature ensures the token hasn't been tampered with.</p>
<p>Security considerations: keep tokens short-lived (15-60 min), use refresh tokens for re-authentication, never store JWTs in localStorage (use httpOnly cookies), and always validate the <code>exp</code> claim server-side.</p>"#),

    // ======================================================================
    // MACHINE LEARNING COURSE
    // ======================================================================

    ("el_ml_1_1", r#"<h2>Linear Regression from Scratch</h2>
<p>Linear regression models the relationship <code>y = wx + b</code> by finding weights <code>w</code> and bias <code>b</code> that minimize the mean squared error (MSE):</p>
<pre><code>MSE = (1/n) Σ (yᵢ - (wxᵢ + b))²</code></pre>
<p>Gradient descent iteratively updates parameters in the direction that reduces error:</p>
<pre><code>w = w - lr * ∂MSE/∂w
b = b - lr * ∂MSE/∂b</code></pre>
<p>The learning rate <code>lr</code> controls step size. Too large: overshooting. Too small: slow convergence. For multiple features, <code>w</code> becomes a vector and the model is <code>y = Xw + b</code> (solvable analytically via the normal equation).</p>"#),

    ("el_ml_1_2", r#"<h2>Logistic Regression</h2>
<p>Despite its name, logistic regression is a <em>classification</em> algorithm. It predicts the probability that an input belongs to class 1:</p>
<pre><code>P(y=1|x) = σ(wx + b) = 1 / (1 + e^(-(wx + b)))</code></pre>
<p>The sigmoid function σ squashes any real number to [0, 1]. We use <strong>binary cross-entropy</strong> as the loss function (not MSE), which is convex and works well with gradient descent.</p>
<p>Decision boundary: predict class 1 if P ≥ 0.5, class 0 otherwise. The threshold can be tuned depending on whether false positives or false negatives are more costly.</p>"#),

    ("el_ml_2_1", r#"<h2>Decision Trees &amp; Random Forests</h2>
<p>A decision tree splits data by asking yes/no questions about features, choosing splits that maximize <strong>information gain</strong> (or minimize Gini impurity). Easy to interpret but prone to overfitting.</p>
<p><strong>Random Forests</strong> fix overfitting by training many trees on random subsets of data and features, then averaging their predictions (bagging). Key hyperparameters:</p>
<ul>
  <li><code>n_estimators</code>: number of trees (more = better, diminishing returns)</li>
  <li><code>max_depth</code>: limits tree depth to prevent overfitting</li>
  <li><code>max_features</code>: number of features considered per split (typically √p)</li>
</ul>
<p>Random forests are a strong baseline for tabular data — fast to train, hard to break, and require minimal tuning.</p>"#),

    ("el_ml_2_2", r#"<h2>K-Means Clustering</h2>
<p>K-Means is an unsupervised algorithm that groups <code>n</code> data points into <code>k</code> clusters. The algorithm:</p>
<ol>
  <li>Initialize <code>k</code> centroids randomly</li>
  <li>Assign each point to the nearest centroid</li>
  <li>Recompute centroids as the mean of assigned points</li>
  <li>Repeat steps 2-3 until convergence</li>
</ol>
<p>K-Means minimizes within-cluster sum of squares (inertia). Choosing <code>k</code>: use the <strong>elbow method</strong> (plot inertia vs k, pick the "bend") or <strong>silhouette score</strong>.</p>
<p>Limitations: assumes spherical clusters of similar size, sensitive to initialization (use k-means++ for better starts), doesn't handle non-convex shapes. For complex cluster shapes, consider DBSCAN.</p>"#),

    ("el_ml_3_1", r#"<h2>Neural Network Architecture</h2>
<p>A neural network is a composition of linear transformations and non-linear activations:</p>
<pre><code>layer(x) = activation(Wx + b)</code></pre>
<p>Common activations: <strong>ReLU</strong> (<code>max(0, x)</code>) for hidden layers, <strong>softmax</strong> for multi-class output, <strong>sigmoid</strong> for binary output.</p>
<p>Architecture choices:</p>
<ul>
  <li><strong>Width</strong>: neurons per layer (more = more capacity)</li>
  <li><strong>Depth</strong>: number of layers (deeper = more abstract features)</li>
  <li><strong>Skip connections</strong>: allow gradients to flow through deep networks (ResNet)</li>
</ul>
<p>The universal approximation theorem says a single hidden layer with enough neurons can approximate any continuous function — but deeper networks learn more efficiently in practice.</p>"#),

    ("el_ml_3_2", r#"<h2>Backpropagation</h2>
<p>Backpropagation computes gradients of the loss with respect to every parameter in the network, using the chain rule of calculus applied layer by layer from output to input.</p>
<p>For a simple two-layer network with loss L:</p>
<pre><code>∂L/∂W₂ = ∂L/∂ŷ · ∂ŷ/∂z₂ · ∂z₂/∂W₂
∂L/∂W₁ = ∂L/∂ŷ · ∂ŷ/∂z₂ · ∂z₂/∂a₁ · ∂a₁/∂z₁ · ∂z₁/∂W₁</code></pre>
<p>Key issues and solutions:</p>
<ul>
  <li><strong>Vanishing gradients</strong>: use ReLU instead of sigmoid/tanh in hidden layers</li>
  <li><strong>Exploding gradients</strong>: gradient clipping, proper weight initialization (He/Xavier)</li>
  <li><strong>Slow convergence</strong>: adaptive optimizers (Adam) adjust learning rate per parameter</li>
</ul>"#),

    ("el_ml_4_1", r#"<h2>Cross-Validation Techniques</h2>
<p>Cross-validation estimates how well a model generalizes to unseen data. <strong>K-Fold CV</strong>:</p>
<ol>
  <li>Split data into <code>k</code> equal folds</li>
  <li>Train on <code>k-1</code> folds, evaluate on the held-out fold</li>
  <li>Repeat <code>k</code> times, rotating the held-out fold</li>
  <li>Average the scores</li>
</ol>
<p>Common values: <code>k=5</code> or <code>k=10</code>. <strong>Stratified</strong> k-fold preserves class distribution in each fold (important for imbalanced data).</p>
<p>Other variants: <strong>Leave-One-Out</strong> (k=n, expensive), <strong>Repeated k-fold</strong> (run k-fold multiple times with different splits), <strong>Time-series split</strong> (never use future data to predict the past).</p>"#),

    ("el_ml_4_2", QUIZ_ML_EVAL),

    // ======================================================================
    // CRYPTOGRAPHY COURSE
    // ======================================================================

    ("el_cry_1_1", r#"<h2>Block Ciphers &amp; AES</h2>
<p>A block cipher encrypts fixed-size blocks of plaintext. <strong>AES</strong> (Advanced Encryption Standard) is the dominant symmetric cipher, operating on 128-bit blocks with 128, 192, or 256-bit keys.</p>
<p>AES alone only encrypts one block. <strong>Modes of operation</strong> extend it to arbitrary-length messages:</p>
<ul>
  <li><strong>ECB</strong>: each block encrypted independently — insecure (reveals patterns)</li>
  <li><strong>CBC</strong>: each block XORed with the previous ciphertext block — requires IV</li>
  <li><strong>CTR</strong>: turns block cipher into stream cipher — parallelizable</li>
  <li><strong>GCM</strong>: CTR mode + authentication tag — the standard choice for authenticated encryption</li>
</ul>
<p>Always use authenticated encryption (AES-GCM or ChaCha20-Poly1305). Encryption without authentication allows ciphertext manipulation.</p>"#),

    ("el_cry_2_1", r#"<h2>RSA Explained</h2>
<p>RSA is based on the difficulty of factoring large semiprimes. Key generation:</p>
<ol>
  <li>Choose two large primes <code>p</code> and <code>q</code></li>
  <li>Compute <code>n = p × q</code> (public modulus)</li>
  <li>Compute <code>φ(n) = (p-1)(q-1)</code></li>
  <li>Choose public exponent <code>e</code> (commonly 65537)</li>
  <li>Compute private exponent <code>d = e⁻¹ mod φ(n)</code></li>
</ol>
<p>Encryption: <code>c = mᵉ mod n</code>. Decryption: <code>m = cᵈ mod n</code>.</p>
<p>RSA is slow (~1000x slower than AES), so in practice it's used to encrypt a symmetric key, which then encrypts the actual data (hybrid encryption). Modern preference is shifting to elliptic curve cryptography for smaller keys and faster operations.</p>"#),

    ("el_cry_2_2", r#"<h2>Elliptic Curve Cryptography</h2>
<p>ECC achieves equivalent security to RSA with much smaller keys. A 256-bit ECC key ≈ 3072-bit RSA key.</p>
<p>An elliptic curve over a finite field: <code>y² = x³ + ax + b (mod p)</code>. The key operation is <strong>point multiplication</strong>: given a point G and scalar k, compute <code>kG</code> (easy). Given <code>G</code> and <code>kG</code>, finding <code>k</code> is the <strong>Elliptic Curve Discrete Logarithm Problem</strong> (hard).</p>
<p>Common curves:</p>
<ul>
  <li><strong>secp256k1</strong>: used by Bitcoin</li>
  <li><strong>Curve25519</strong>: designed by Daniel Bernstein, used in X25519 key exchange and Ed25519 signatures</li>
  <li><strong>P-256 (secp256r1)</strong>: NIST standard, widely deployed in TLS</li>
</ul>"#),

    ("el_cry_3_1", r#"<h2>SHA-256 &amp; BLAKE2</h2>
<p>Cryptographic hash functions map arbitrary data to a fixed-size digest. Properties:</p>
<ul>
  <li><strong>Preimage resistance</strong>: given <code>h</code>, infeasible to find <code>m</code> such that <code>H(m) = h</code></li>
  <li><strong>Second preimage resistance</strong>: given <code>m₁</code>, infeasible to find <code>m₂ ≠ m₁</code> with the same hash</li>
  <li><strong>Collision resistance</strong>: infeasible to find any <code>m₁ ≠ m₂</code> with the same hash</li>
</ul>
<p><strong>SHA-256</strong> produces a 256-bit digest. Used in Bitcoin, TLS certificates, and content addressing. Merkle trees chain hashes for efficient verification of large datasets.</p>
<p><strong>BLAKE2</strong> is faster than SHA-256 while maintaining equivalent security. <strong>BLAKE3</strong> (used by iroh in this app) is even faster, with built-in parallelism and a tree structure.</p>"#),

    ("el_cry_3_2", r#"<h2>Digital Signatures with Ed25519</h2>
<p>A digital signature proves that a message was created by the holder of a specific private key, without revealing the key itself.</p>
<p><strong>Ed25519</strong> uses the Edwards curve Curve25519. It produces 64-byte signatures with 32-byte keys. Advantages:</p>
<ul>
  <li>Fast: ~70,000 signatures/second on commodity hardware</li>
  <li>Deterministic: same message + key always produces the same signature (no random nonce needed)</li>
  <li>Resistant to side-channel attacks by design</li>
</ul>
<p>Verification: given (message, signature, public_key), anyone can check validity without the private key. This is the foundation of blockchain transactions, code signing, and certificate chains.</p>"#),

    ("el_cry_4_1", r#"<h2>Introduction to ZK Proofs</h2>
<p>A zero-knowledge proof lets you prove you know something without revealing what you know. Three properties:</p>
<ul>
  <li><strong>Completeness</strong>: if the statement is true, the verifier will be convinced</li>
  <li><strong>Soundness</strong>: if the statement is false, the prover can't convince the verifier (except with negligible probability)</li>
  <li><strong>Zero-knowledge</strong>: the verifier learns nothing beyond the truth of the statement</li>
</ul>
<p><strong>ZK-SNARKs</strong>: Succinct Non-interactive Arguments of Knowledge. Proofs are tiny (~200 bytes) and fast to verify, but require a trusted setup. Used in Zcash.</p>
<p><strong>ZK-STARKs</strong>: Scalable Transparent Arguments of Knowledge. No trusted setup, quantum-resistant, but larger proofs. Used in StarkNet.</p>
<p>Applications: private transactions, identity verification (prove you're over 18 without revealing your age), verifiable computation.</p>"#),

    // ======================================================================
    // UX DESIGN COURSE
    // ======================================================================

    ("el_ux_1_1", r#"<h2>Planning User Interviews</h2>
<p>User interviews are semi-structured conversations that uncover needs, behaviors, and pain points. Planning steps:</p>
<ol>
  <li><strong>Define research questions</strong>: what do you want to learn? (not what to ask — those are different)</li>
  <li><strong>Recruit participants</strong>: 5-8 users per segment is usually sufficient for qualitative insights</li>
  <li><strong>Write an interview guide</strong>: open-ended questions, ordered from broad to specific</li>
  <li><strong>Prepare logistics</strong>: recording consent, note-taker, 30-60 min sessions</li>
</ol>
<p>Key techniques: ask "why" and "how" (not just "what"), follow up on unexpected answers, avoid leading questions ("Don't you think X is confusing?" → "How did you find X?").</p>"#),

    ("el_ux_1_2", r#"<h2>Creating Personas</h2>
<p>Personas are fictional archetypes that represent key user segments. A good persona includes:</p>
<ul>
  <li><strong>Demographics</strong>: name, age, role, tech comfort level</li>
  <li><strong>Goals</strong>: what they're trying to achieve</li>
  <li><strong>Frustrations</strong>: current pain points</li>
  <li><strong>Behaviors</strong>: how they currently solve the problem</li>
  <li><strong>Context</strong>: when and where they interact with the product</li>
</ul>
<p>Personas should be based on research, not assumptions. Aim for 3-5 personas — enough to cover your user base without overwhelming decision-making. The primary persona's needs should drive core design decisions.</p>"#),

    ("el_ux_2_1", r#"<h2>Card Sorting Workshop</h2>
<p>Card sorting helps design intuitive information architecture by understanding how users categorize content.</p>
<p><strong>Open sort</strong>: participants group cards and name the groups themselves. Reveals their mental model.</p>
<p><strong>Closed sort</strong>: participants sort cards into predefined categories. Tests an existing structure.</p>
<p>Process:</p>
<ol>
  <li>Write each content item on a card (30-60 cards is ideal)</li>
  <li>Ask 15-20 participants to sort independently</li>
  <li>Analyze with a similarity matrix — items frequently grouped together should be near each other in your navigation</li>
</ol>
<p>Tools: OptimalSort (remote), physical sticky notes (in-person). Results inform your sitemap and navigation labels.</p>"#),

    ("el_ux_2_2", r#"<h2>Navigation Patterns</h2>
<p>Navigation is how users move through your product. Common patterns:</p>
<ul>
  <li><strong>Top nav</strong>: horizontal bar for primary sections (works for 3-7 items)</li>
  <li><strong>Side nav</strong>: vertical sidebar for apps with many sections (collapsible for mobile)</li>
  <li><strong>Tabs</strong>: for switching between views of the same content</li>
  <li><strong>Breadcrumbs</strong>: show location in hierarchy, allow backtracking</li>
  <li><strong>Bottom nav</strong> (mobile): 3-5 primary destinations</li>
</ul>
<p>Design principles: keep navigation consistent across pages, highlight the current location, use clear labels (nouns, not verbs), and limit depth to 3 levels. Progressive disclosure: show top-level options first, reveal details on demand.</p>"#),

    ("el_ux_3_1", r#"<h2>Low-Fidelity Wireframes</h2>
<p>Low-fi wireframes are rough sketches that focus on layout and content hierarchy, deliberately avoiding visual design details.</p>
<p>Tools: pen and paper (fastest), Balsamiq (digital sketchy look), or any whiteboard. The rough aesthetic is intentional — it invites feedback on structure rather than aesthetics.</p>
<p>What to include: content blocks, navigation, key interactions. What to leave out: colors, images, exact copy, fonts.</p>
<p>Process: sketch multiple layout options quickly (5-10 minutes each), get feedback, iterate. Paper wireframes can be tested with users via "paper prototyping" — you play the computer, swapping pages as the user "clicks."</p>"#),

    ("el_ux_3_2", r#"<h2>Interactive Prototyping</h2>
<p>Interactive prototypes simulate the user experience with clickable screens. They range from simple click-through mockups to high-fidelity simulations.</p>
<p>Tools: Figma (industry standard), Sketch + InVision, or code-based prototypes for complex interactions.</p>
<p>Fidelity levels:</p>
<ul>
  <li><strong>Low</strong>: linked wireframes, test flow and structure</li>
  <li><strong>Medium</strong>: styled screens, test visual hierarchy and content</li>
  <li><strong>High</strong>: pixel-perfect with animations, test micro-interactions</li>
</ul>
<p>Prototype only what you need to test. A common mistake is over-investing in prototype fidelity before validating the core concept. Test early and often — 5 users per round, iterate between rounds.</p>"#),

    ("el_ux_4_1", r#"<h2>Color Theory for Screens</h2>
<p>Color choices affect usability, accessibility, and emotional response. Key concepts:</p>
<ul>
  <li><strong>HSL model</strong>: Hue (color), Saturation (intensity), Lightness (brightness) — more intuitive than RGB for design</li>
  <li><strong>Color harmony</strong>: complementary (opposite on wheel), analogous (adjacent), triadic (evenly spaced)</li>
  <li><strong>60-30-10 rule</strong>: 60% dominant color, 30% secondary, 10% accent</li>
</ul>
<p>Accessibility requirements (WCAG 2.1): normal text needs 4.5:1 contrast ratio against background, large text needs 3:1. Never use color alone to convey information — add icons or text labels.</p>
<p>Dark mode considerations: don't just invert colors. Use dark gray (#121212) instead of pure black, reduce saturation of colors, and test contrast ratios separately.</p>"#),

    ("el_ux_4_2", r#"<h2>Typography Best Practices</h2>
<p>Typography is the most important design skill — text makes up 80-90% of most interfaces.</p>
<p>Key principles:</p>
<ul>
  <li><strong>Type scale</strong>: use a consistent ratio (e.g., 1.25 or 1.333) between sizes. Example: 12, 14, 16, 20, 24, 32px</li>
  <li><strong>Line height</strong>: 1.4-1.6 for body text, 1.1-1.3 for headings</li>
  <li><strong>Line length</strong>: 45-75 characters per line for readability</li>
  <li><strong>Font pairing</strong>: combine a serif and sans-serif, or two weights of the same family</li>
</ul>
<p>System font stacks (<code>system-ui, -apple-system, sans-serif</code>) load instantly and match the platform's native feel. For custom fonts, use <code>font-display: swap</code> to avoid invisible text during loading.</p>"#),

    // ======================================================================
    // INTERACTIVE ELEMENTS (HTML with embedded interactive descriptions)
    // ======================================================================

    ("el_algo_3_5", r#"<h2>Graph Traversal Simulation</h2>
<p>This interactive simulation lets you visualize how BFS and DFS explore a graph step by step.</p>
<h3>How It Works</h3>
<ol>
  <li><strong>Build the graph</strong>: Click to add nodes, drag between nodes to create edges</li>
  <li><strong>Pick a start node</strong>: Click any node to begin the traversal</li>
  <li><strong>Choose an algorithm</strong>: Toggle between BFS (breadth-first) and DFS (depth-first)</li>
  <li><strong>Step through</strong>: Use the "Next Step" button to advance one step at a time, or "Auto-play" to animate</li>
</ol>
<h3>What to Observe</h3>
<ul>
  <li><strong>BFS</strong>: Nodes are visited level by level (concentric rings outward). The queue is shown below.</li>
  <li><strong>DFS</strong>: Nodes are visited deeply along one branch before backtracking. The stack is shown below.</li>
  <li>Notice how BFS finds shortest paths in unweighted graphs, while DFS explores as deep as possible first.</li>
</ul>
<p>Try creating a graph with cycles and observe how the <code>visited</code> set prevents infinite loops in both algorithms.</p>"#),

    ("el_web_1_3", r#"<h2>CSS Layout Workshop</h2>
<p>Practice building responsive layouts using Flexbox and CSS Grid in this hands-on workshop.</p>
<h3>Exercise 1: Flexbox Navigation Bar</h3>
<p>Create a horizontal navigation bar with the logo on the left and links on the right:</p>
<pre><code>/* Your goal: */
.navbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1rem 2rem;
}

.nav-links {
  display: flex;
  gap: 1.5rem;
}</code></pre>
<h3>Exercise 2: Grid Product Cards</h3>
<p>Build a responsive card grid that shows 1 column on mobile, 2 on tablet, 3 on desktop:</p>
<pre><code>.card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 1.5rem;
}</code></pre>
<h3>Exercise 3: Holy Grail Layout</h3>
<p>Combine Grid and Flexbox to build the classic "holy grail" layout: header, footer, main content area with two sidebars. The main content should be fluid, sidebars fixed-width.</p>"#),

    ("el_ml_2_4", r#"<h2>K-Means Interactive Visualization</h2>
<p>Explore how the K-Means algorithm works by interacting with a 2D scatter plot.</p>
<h3>Controls</h3>
<ul>
  <li><strong>Click on the canvas</strong> to place data points</li>
  <li><strong>Set K</strong>: choose the number of clusters (2-6)</li>
  <li><strong>Initialize</strong>: randomly place K centroids (shown as larger colored circles)</li>
  <li><strong>Step</strong>: run one iteration — assign points to nearest centroid, then recompute centroids</li>
  <li><strong>Run to convergence</strong>: auto-iterate until centroids stop moving</li>
</ul>
<h3>Experiments to Try</h3>
<ol>
  <li>Place 3 well-separated clusters and run K-Means with K=3. Does it find them?</li>
  <li>Try K=2 on the same 3-cluster data. What happens?</li>
  <li>Place points in a ring shape and try K=2. Does K-Means handle non-convex shapes?</li>
  <li>Run the algorithm multiple times with the same data. Do you always get the same result?</li>
</ol>
<p>This demonstrates why K-Means is sensitive to initialization and why <strong>k-means++</strong> uses smarter initial centroid placement.</p>"#),

    ("el_cry_4_2", r#"<h2>ZK Proof Interactive Demo</h2>
<p>Experience the magic of zero-knowledge proofs through a classic example: the "Ali Baba Cave" protocol.</p>
<h3>The Setup</h3>
<p>There is a circular cave with a locked door in the middle. You (the prover) claim to know the secret word that opens the door. I (the verifier) want proof without learning the secret.</p>
<h3>The Protocol</h3>
<ol>
  <li>You enter the cave and choose path A or path B (I don't watch)</li>
  <li>I shout which path I want you to come out from</li>
  <li>If you know the secret, you can always come out the correct path (through the door if needed)</li>
  <li>If you DON'T know the secret, you can only come out the correct path 50% of the time</li>
  <li>We repeat N times — after 20 rounds, I'm 99.9999% confident you know the secret</li>
</ol>
<h3>Interact</h3>
<p>Click "Enter Cave" to start a round. Choose a path. The verifier will challenge you. Try both scenarios: knowing the secret and not knowing it. Observe how the probability of faking it drops exponentially with each round.</p>
<p>This is exactly the structure of real ZK-SNARKs and ZK-STARKs, just with mathematical commitments instead of cave paths.</p>"#),

    // ======================================================================
    // ASSESSMENT ELEMENTS (QuizDefinition JSON — comprehensive end-of-course)
    // ======================================================================

    ("el_algo_5_4", ASSESSMENT_ALGO),
    ("el_web_5_5",  ASSESSMENT_WEB),
    ("el_ml_4_3",   ASSESSMENT_ML),
    ("el_cry_4_3",  ASSESSMENT_CRYPTO),
    ("el_ux_4_4",   ASSESSMENT_UX),

    // ======================================================================
    // OBJECTIVE SINGLE MCQ ELEMENTS
    // ======================================================================

    ("el_algo_2_5", MCQ_SINGLE_ALGO),
    ("el_web_2_3",  MCQ_SINGLE_WEB),
    ("el_cry_1_3",  MCQ_SINGLE_CRYPTO),
    ("el_ux_2_3",   MCQ_SINGLE_UX),

    // ======================================================================
    // OBJECTIVE MULTI MCQ ELEMENTS
    // ======================================================================

    ("el_algo_4_4", MCQ_MULTI_ALGO),
    ("el_ml_2_3",   MCQ_MULTI_ML),
    ("el_cry_3_3",  MCQ_MULTI_CRYPTO),

    // ======================================================================
    // SUBJECTIVE MCQ ELEMENTS
    // ======================================================================

    ("el_web_4_3",  MCQ_SUBJ_WEB),
    ("el_cry_2_3",  MCQ_SUBJ_CRYPTO),
    ("el_ux_3_4",   MCQ_SUBJ_UX),

    // ======================================================================
    // ESSAY ELEMENTS
    // ======================================================================

    ("el_web_2_4",  ESSAY_WEB),
    ("el_ml_3_3",   ESSAY_ML),
    ("el_ux_1_4",   ESSAY_UX),
];

// ---------------------------------------------------------------------------
// Quiz content (QuizDefinition JSON matching QuizEngine.vue expectations)
// ---------------------------------------------------------------------------

const QUIZ_COMPLEXITY: &str = r#"{
  "title": "Complexity Analysis Quiz",
  "pass_threshold": 0.66,
  "questions": [
    {
      "id": "q1",
      "type": "single_choice",
      "prompt": "What is the time complexity of binary search on a sorted array of n elements?",
      "options": ["O(1)", "O(log n)", "O(n)", "O(n log n)"],
      "correct_indices": [1],
      "explanation": "Binary search halves the search space each step, giving O(log n).",
      "points": 1,
      "difficulty": 1
    },
    {
      "id": "q2",
      "type": "single_choice",
      "prompt": "A function has two nested loops, each iterating n times. What is its time complexity?",
      "options": ["O(n)", "O(n log n)", "O(n²)", "O(2ⁿ)"],
      "correct_indices": [2],
      "explanation": "Two nested loops of n iterations each: n × n = O(n²).",
      "points": 1,
      "difficulty": 1
    },
    {
      "id": "q3",
      "type": "true_false",
      "prompt": "O(3n² + 5n) simplifies to O(n²).",
      "options": ["True", "False"],
      "correct_indices": [0],
      "explanation": "Big-O drops constants and lower-order terms.",
      "points": 1,
      "difficulty": 1
    }
  ]
}"#;

const QUIZ_DATA_STRUCTURES: &str = r#"{
  "title": "Data Structures Quiz",
  "pass_threshold": 0.66,
  "questions": [
    {
      "id": "q1",
      "type": "single_choice",
      "prompt": "Which data structure uses LIFO (Last In, First Out) ordering?",
      "options": ["Queue", "Stack", "Linked List", "Array"],
      "correct_indices": [1],
      "explanation": "A stack follows LIFO — the last element pushed is the first one popped.",
      "points": 1,
      "difficulty": 1
    },
    {
      "id": "q2",
      "type": "single_choice",
      "prompt": "What is the time complexity of accessing an element by index in an array?",
      "options": ["O(1)", "O(log n)", "O(n)", "O(n²)"],
      "correct_indices": [0],
      "explanation": "Arrays provide O(1) random access because elements are stored contiguously.",
      "points": 1,
      "difficulty": 1
    },
    {
      "id": "q3",
      "type": "single_choice",
      "prompt": "What is the main advantage of a linked list over an array?",
      "options": ["Faster random access", "O(1) insertion at a known position", "Less memory usage", "Better cache locality"],
      "correct_indices": [1],
      "explanation": "Linked lists can insert/delete at a known node in O(1) without shifting elements.",
      "points": 1,
      "difficulty": 1
    }
  ]
}"#;

const QUIZ_TREES_GRAPHS: &str = r#"{
  "title": "Trees & Graphs Quiz",
  "pass_threshold": 0.66,
  "questions": [
    {
      "id": "q1",
      "type": "single_choice",
      "prompt": "Which traversal of a BST produces elements in sorted order?",
      "options": ["Pre-order", "In-order", "Post-order", "Level-order"],
      "correct_indices": [1],
      "explanation": "In-order traversal (left → root → right) visits BST nodes in ascending order.",
      "points": 1,
      "difficulty": 1
    },
    {
      "id": "q2",
      "type": "single_choice",
      "prompt": "Which algorithm guarantees the shortest path in an unweighted graph?",
      "options": ["DFS", "BFS", "Dijkstra's", "Bellman-Ford"],
      "correct_indices": [1],
      "explanation": "BFS explores nodes level by level, guaranteeing shortest paths in unweighted graphs.",
      "points": 1,
      "difficulty": 1
    },
    {
      "id": "q3",
      "type": "true_false",
      "prompt": "A tree with n nodes always has exactly n-1 edges.",
      "options": ["True", "False"],
      "correct_indices": [0],
      "explanation": "A tree is a connected acyclic graph, and any such graph with n nodes has n-1 edges.",
      "points": 1,
      "difficulty": 1
    }
  ]
}"#;

const QUIZ_SORTING: &str = r#"{
  "title": "Sorting Algorithms Quiz",
  "pass_threshold": 0.66,
  "questions": [
    {
      "id": "q1",
      "type": "single_choice",
      "prompt": "What is the average-case time complexity of Quick Sort?",
      "options": ["O(n)", "O(n log n)", "O(n²)", "O(log n)"],
      "correct_indices": [1],
      "explanation": "Quick Sort averages O(n log n) with good pivot selection, though worst case is O(n²).",
      "points": 1,
      "difficulty": 1
    },
    {
      "id": "q2",
      "type": "single_choice",
      "prompt": "Which sorting algorithm is stable and always O(n log n)?",
      "options": ["Quick Sort", "Selection Sort", "Merge Sort", "Heap Sort"],
      "correct_indices": [2],
      "explanation": "Merge Sort is stable and guarantees O(n log n) regardless of input.",
      "points": 1,
      "difficulty": 1
    },
    {
      "id": "q3",
      "type": "true_false",
      "prompt": "Bubble Sort's best case is O(n) when the array is already sorted.",
      "options": ["True", "False"],
      "correct_indices": [0],
      "explanation": "With early termination (no swaps in a pass), Bubble Sort detects a sorted array in O(n).",
      "points": 1,
      "difficulty": 1
    }
  ]
}"#;

const QUIZ_HASH_TABLES: &str = r#"{
  "title": "Hash Tables Quiz",
  "pass_threshold": 0.66,
  "questions": [
    {
      "id": "q1",
      "type": "single_choice",
      "prompt": "What is the average time complexity for lookup in a hash table?",
      "options": ["O(1)", "O(log n)", "O(n)", "O(n²)"],
      "correct_indices": [0],
      "explanation": "With a good hash function and reasonable load factor, hash table lookups are O(1) on average.",
      "points": 1,
      "difficulty": 1
    },
    {
      "id": "q2",
      "type": "single_choice",
      "prompt": "Which collision resolution strategy stores all entries in the array itself?",
      "options": ["Chaining", "Open addressing", "Separate chaining", "Bucket sorting"],
      "correct_indices": [1],
      "explanation": "Open addressing probes for empty slots within the array rather than using external chains.",
      "points": 1,
      "difficulty": 1
    },
    {
      "id": "q3",
      "type": "single_choice",
      "prompt": "What happens when a hash table's load factor gets too high?",
      "options": ["Keys are deleted", "The table resizes and rehashes", "Lookups become O(1)", "Nothing changes"],
      "correct_indices": [1],
      "explanation": "When load factor exceeds the threshold (~0.75), the table doubles capacity and rehashes all entries.",
      "points": 1,
      "difficulty": 1
    }
  ]
}"#;

const QUIZ_ML_EVAL: &str = r#"{
  "title": "Model Evaluation Quiz",
  "pass_threshold": 0.66,
  "questions": [
    {
      "id": "q1",
      "type": "single_choice",
      "prompt": "In 5-fold cross-validation, how many times is each data point used for testing?",
      "options": ["0", "1", "5", "Depends on the data"],
      "correct_indices": [1],
      "explanation": "Each data point appears in exactly one test fold, so it's tested exactly once.",
      "points": 1,
      "difficulty": 1
    },
    {
      "id": "q2",
      "type": "single_choice",
      "prompt": "Which metric is most appropriate for an imbalanced classification dataset?",
      "options": ["Accuracy", "F1 Score", "Mean Squared Error", "R² Score"],
      "correct_indices": [1],
      "explanation": "F1 Score balances precision and recall, making it more meaningful than accuracy when classes are imbalanced.",
      "points": 1,
      "difficulty": 1
    },
    {
      "id": "q3",
      "type": "single_choice",
      "prompt": "A model with high training accuracy but low test accuracy is likely:",
      "options": ["Underfitting", "Overfitting", "Well-calibrated", "Too simple"],
      "correct_indices": [1],
      "explanation": "High train / low test accuracy indicates the model memorized training data rather than learning generalizable patterns.",
      "points": 1,
      "difficulty": 1
    }
  ]
}"#;

// ---------------------------------------------------------------------------
// Assessment content (QuizDefinition JSON — end-of-course comprehensive exams)
// ---------------------------------------------------------------------------

const ASSESSMENT_ALGO: &str = r#"{
  "title": "Algorithms & Data Structures Final Assessment",
  "description": "Comprehensive assessment covering all topics from the course.",
  "pass_threshold": 0.7,
  "time_limit_seconds": 1800,
  "questions": [
    {
      "id": "a1",
      "type": "single_choice",
      "prompt": "What is the time complexity of inserting an element at the beginning of a dynamic array (Vec)?",
      "options": ["O(1)", "O(log n)", "O(n)", "O(n²)"],
      "correct_indices": [2],
      "explanation": "Inserting at the beginning requires shifting all existing elements, which is O(n).",
      "points": 2,
      "difficulty": 2
    },
    {
      "id": "a2",
      "type": "single_choice",
      "prompt": "Which traversal produces a topological ordering of a DAG?",
      "options": ["BFS", "DFS post-order (reversed)", "In-order", "Level-order"],
      "correct_indices": [1],
      "explanation": "Reverse DFS post-order gives a valid topological sort of a directed acyclic graph.",
      "points": 2,
      "difficulty": 2
    },
    {
      "id": "a3",
      "type": "single_choice",
      "prompt": "Which sorting algorithm has the best worst-case time complexity?",
      "options": ["Quick Sort — O(n log n)", "Merge Sort — O(n log n)", "Bubble Sort — O(n²)", "Selection Sort — O(n²)"],
      "correct_indices": [1],
      "explanation": "Merge Sort always runs in O(n log n), while Quick Sort degrades to O(n²) in the worst case.",
      "points": 2,
      "difficulty": 2
    },
    {
      "id": "a4",
      "type": "true_false",
      "prompt": "A hash table with chaining can have a load factor greater than 1.0.",
      "options": ["True", "False"],
      "correct_indices": [0],
      "explanation": "With chaining, multiple entries can share a bucket, so the load factor (n/m) can exceed 1.0.",
      "points": 2,
      "difficulty": 2
    },
    {
      "id": "a5",
      "type": "single_choice",
      "prompt": "What is the space complexity of a recursive DFS on a graph with V vertices?",
      "options": ["O(1)", "O(log V)", "O(V)", "O(V + E)"],
      "correct_indices": [2],
      "explanation": "The recursion stack can grow up to O(V) in the worst case (e.g., a linear graph).",
      "points": 2,
      "difficulty": 3
    }
  ]
}"#;

const ASSESSMENT_WEB: &str = r#"{
  "title": "Full-Stack Web Development Final Assessment",
  "description": "End-of-course assessment covering HTML/CSS, JavaScript, TypeScript, Vue, and Rust backend.",
  "pass_threshold": 0.7,
  "time_limit_seconds": 1800,
  "questions": [
    {
      "id": "a1",
      "type": "single_choice",
      "prompt": "Which CSS property creates a flex container?",
      "options": ["flex: 1", "display: flex", "position: flex", "flex-direction: row"],
      "correct_indices": [1],
      "explanation": "display: flex turns an element into a flex container. The other properties configure flex behavior.",
      "points": 2,
      "difficulty": 1
    },
    {
      "id": "a2",
      "type": "single_choice",
      "prompt": "What does 'await' do in an async function?",
      "options": ["Blocks the entire JavaScript thread", "Pauses the function until the Promise resolves", "Creates a new thread", "Catches errors automatically"],
      "correct_indices": [1],
      "explanation": "await pauses the async function execution (not the thread) until the Promise settles, then resumes with the resolved value.",
      "points": 2,
      "difficulty": 2
    },
    {
      "id": "a3",
      "type": "single_choice",
      "prompt": "In Vue 3, what is the purpose of ref()?",
      "options": ["To reference a DOM element", "To create a reactive primitive value", "To define a component ref", "To import a module"],
      "correct_indices": [1],
      "explanation": "ref() creates a reactive wrapper around a value (accessed via .value), enabling Vue's reactivity tracking.",
      "points": 2,
      "difficulty": 2
    },
    {
      "id": "a4",
      "type": "true_false",
      "prompt": "JWT tokens should be stored in localStorage for maximum security.",
      "options": ["True", "False"],
      "correct_indices": [1],
      "explanation": "localStorage is vulnerable to XSS attacks. httpOnly cookies are the recommended storage for JWT tokens.",
      "points": 2,
      "difficulty": 2
    },
    {
      "id": "a5",
      "type": "single_choice",
      "prompt": "In TypeScript, what does the 'unknown' type represent?",
      "options": ["Same as 'any' — no type checking", "A type that must be narrowed before use", "An undefined variable", "A compile error"],
      "correct_indices": [1],
      "explanation": "unknown is the type-safe counterpart of any — you must narrow it (e.g., with type guards) before accessing properties.",
      "points": 2,
      "difficulty": 2
    }
  ]
}"#;

const ASSESSMENT_ML: &str = r#"{
  "title": "Machine Learning Foundations Final Assessment",
  "description": "Comprehensive assessment on regression, classification, neural networks, and evaluation.",
  "pass_threshold": 0.7,
  "time_limit_seconds": 1800,
  "questions": [
    {
      "id": "a1",
      "type": "single_choice",
      "prompt": "What loss function is used for binary classification in logistic regression?",
      "options": ["Mean Squared Error", "Binary Cross-Entropy", "Hinge Loss", "KL Divergence"],
      "correct_indices": [1],
      "explanation": "Binary cross-entropy (log loss) is the standard loss for logistic regression, as it's convex and well-suited for probability outputs.",
      "points": 2,
      "difficulty": 2
    },
    {
      "id": "a2",
      "type": "single_choice",
      "prompt": "What does the 'k' in k-fold cross-validation represent?",
      "options": ["Number of features", "Number of folds/splits", "Number of training epochs", "Number of classes"],
      "correct_indices": [1],
      "explanation": "k is the number of equal-sized folds the dataset is split into; the model is trained k times, each time using a different fold for validation.",
      "points": 2,
      "difficulty": 1
    },
    {
      "id": "a3",
      "type": "single_choice",
      "prompt": "Which activation function is most commonly used in hidden layers of modern neural networks?",
      "options": ["Sigmoid", "Tanh", "ReLU", "Softmax"],
      "correct_indices": [2],
      "explanation": "ReLU (max(0, x)) is preferred for hidden layers because it doesn't suffer from vanishing gradients like sigmoid/tanh.",
      "points": 2,
      "difficulty": 2
    },
    {
      "id": "a4",
      "type": "true_false",
      "prompt": "Random Forests are less prone to overfitting than individual decision trees.",
      "options": ["True", "False"],
      "correct_indices": [0],
      "explanation": "By averaging predictions from many trees trained on random subsets (bagging), Random Forests reduce overfitting compared to a single deep tree.",
      "points": 2,
      "difficulty": 2
    },
    {
      "id": "a5",
      "type": "single_choice",
      "prompt": "What is the primary purpose of the Adam optimizer?",
      "options": ["To prevent overfitting", "To adaptively adjust learning rates per parameter", "To initialize weights", "To normalize input data"],
      "correct_indices": [1],
      "explanation": "Adam combines momentum and RMSProp to maintain per-parameter adaptive learning rates, accelerating convergence.",
      "points": 2,
      "difficulty": 3
    }
  ]
}"#;

const ASSESSMENT_CRYPTO: &str = r#"{
  "title": "Applied Cryptography Final Assessment",
  "description": "End-of-course assessment covering symmetric, asymmetric, hashing, signatures, and ZK proofs.",
  "pass_threshold": 0.7,
  "time_limit_seconds": 1800,
  "questions": [
    {
      "id": "a1",
      "type": "single_choice",
      "prompt": "Why is ECB mode considered insecure?",
      "options": ["It's too slow", "Identical plaintext blocks produce identical ciphertext blocks", "It doesn't use a key", "It only works with small messages"],
      "correct_indices": [1],
      "explanation": "ECB encrypts each block independently, so identical input blocks produce identical output, revealing patterns in the data.",
      "points": 2,
      "difficulty": 2
    },
    {
      "id": "a2",
      "type": "single_choice",
      "prompt": "What advantage does Ed25519 have over RSA for digital signatures?",
      "options": ["Larger key sizes", "Smaller keys with equivalent security", "No mathematical basis", "Slower verification"],
      "correct_indices": [1],
      "explanation": "Ed25519 achieves comparable security to RSA-3072 with just 32-byte keys, plus it's faster and deterministic.",
      "points": 2,
      "difficulty": 2
    },
    {
      "id": "a3",
      "type": "true_false",
      "prompt": "ZK-STARKs require a trusted setup ceremony.",
      "options": ["True", "False"],
      "correct_indices": [1],
      "explanation": "ZK-STARKs are 'transparent' — they don't require a trusted setup, unlike ZK-SNARKs which do.",
      "points": 2,
      "difficulty": 2
    },
    {
      "id": "a4",
      "type": "single_choice",
      "prompt": "What does 'authenticated encryption' provide that plain encryption does not?",
      "options": ["Faster encryption", "Integrity verification (tamper detection)", "Smaller ciphertext", "Key generation"],
      "correct_indices": [1],
      "explanation": "Authenticated encryption (like AES-GCM) includes an authentication tag that detects any modification to the ciphertext.",
      "points": 2,
      "difficulty": 2
    },
    {
      "id": "a5",
      "type": "single_choice",
      "prompt": "In a Merkle tree, what does changing a single leaf node affect?",
      "options": ["Only that leaf's hash", "All hashes on the path to the root", "The entire tree must be recomputed", "Nothing — leaves are independent"],
      "correct_indices": [1],
      "explanation": "Changing a leaf updates its hash, which changes its parent, which changes its grandparent, all the way up to the root — but only O(log n) hashes total.",
      "points": 2,
      "difficulty": 3
    }
  ]
}"#;

const ASSESSMENT_UX: &str = r#"{
  "title": "UX Design Final Assessment",
  "description": "Comprehensive assessment on user research, IA, wireframing, and visual design.",
  "pass_threshold": 0.7,
  "time_limit_seconds": 1200,
  "questions": [
    {
      "id": "a1",
      "type": "single_choice",
      "prompt": "How many users are typically sufficient for a qualitative usability test?",
      "options": ["1-2", "5-8", "20-30", "100+"],
      "correct_indices": [1],
      "explanation": "Nielsen's research shows that 5-8 users uncover approximately 80-85% of usability issues in qualitative studies.",
      "points": 2,
      "difficulty": 1
    },
    {
      "id": "a2",
      "type": "single_choice",
      "prompt": "What is the purpose of card sorting in UX design?",
      "options": ["To test visual design", "To understand how users categorize information", "To measure page load time", "To conduct A/B tests"],
      "correct_indices": [1],
      "explanation": "Card sorting reveals users' mental models for content organization, informing information architecture decisions.",
      "points": 2,
      "difficulty": 1
    },
    {
      "id": "a3",
      "type": "single_choice",
      "prompt": "What contrast ratio does WCAG 2.1 AA require for normal text?",
      "options": ["2:1", "3:1", "4.5:1", "7:1"],
      "correct_indices": [2],
      "explanation": "WCAG 2.1 AA requires at least 4.5:1 for normal text and 3:1 for large text (18px+ or 14px+ bold).",
      "points": 2,
      "difficulty": 2
    },
    {
      "id": "a4",
      "type": "true_false",
      "prompt": "Low-fidelity wireframes should include final colors and typography.",
      "options": ["True", "False"],
      "correct_indices": [1],
      "explanation": "Low-fi wireframes deliberately omit visual design details to focus feedback on layout, structure, and content hierarchy.",
      "points": 2,
      "difficulty": 1
    },
    {
      "id": "a5",
      "type": "single_choice",
      "prompt": "What is the recommended line length for body text readability?",
      "options": ["20-30 characters", "45-75 characters", "100-120 characters", "No limit"],
      "correct_indices": [1],
      "explanation": "Research consistently shows 45-75 characters per line is optimal for comfortable reading speed and comprehension.",
      "points": 2,
      "difficulty": 2
    }
  ]
}"#;

// ---------------------------------------------------------------------------
// Objective Single MCQ content (McqContent JSON)
// ---------------------------------------------------------------------------

const MCQ_SINGLE_ALGO: &str = r#"{
  "question": "You need to frequently insert and remove elements from both ends of a collection. Which data structure is most appropriate?",
  "options": [
    { "id": "a", "text": "Array (Vec)" },
    { "id": "b", "text": "Singly Linked List" },
    { "id": "c", "text": "Double-ended Queue (VecDeque)" },
    { "id": "d", "text": "Hash Map" }
  ],
  "correct_option_index": 2,
  "explanation": "A VecDeque (double-ended queue) provides O(1) amortized insertion and removal at both the front and back, making it ideal for this use case."
}"#;

const MCQ_SINGLE_WEB: &str = r#"{
  "question": "Which of the following is NOT a valid way to declare a variable in modern JavaScript (ES6+)?",
  "options": [
    { "id": "a", "text": "let count = 0" },
    { "id": "b", "text": "const name = 'Alice'" },
    { "id": "c", "text": "var total = 100" },
    { "id": "d", "text": "def value = 42" }
  ],
  "correct_option_index": 3,
  "explanation": "'def' is a keyword in Python and Ruby, not JavaScript. JavaScript uses 'let', 'const', and 'var' for variable declarations."
}"#;

const MCQ_SINGLE_CRYPTO: &str = r#"{
  "question": "Which AES mode of operation provides authenticated encryption?",
  "options": [
    { "id": "a", "text": "ECB (Electronic Codebook)" },
    { "id": "b", "text": "CBC (Cipher Block Chaining)" },
    { "id": "c", "text": "CTR (Counter)" },
    { "id": "d", "text": "GCM (Galois/Counter Mode)" }
  ],
  "correct_option_index": 3,
  "explanation": "GCM combines CTR mode encryption with a Galois field MAC, providing both confidentiality and integrity (authenticated encryption). ECB, CBC, and CTR only provide confidentiality."
}"#;

const MCQ_SINGLE_UX: &str = r#"{
  "question": "In information architecture, what does a 'closed card sort' test?",
  "options": [
    { "id": "a", "text": "Whether users can create their own categories" },
    { "id": "b", "text": "Whether users can sort cards into predefined categories" },
    { "id": "c", "text": "The visual design of navigation elements" },
    { "id": "d", "text": "Page load performance metrics" }
  ],
  "correct_option_index": 1,
  "explanation": "In a closed card sort, participants sort content into predefined categories you provide, testing whether your existing structure matches users' mental models."
}"#;

// ---------------------------------------------------------------------------
// Objective Multi MCQ content (McqContent JSON)
// ---------------------------------------------------------------------------

const MCQ_MULTI_ALGO: &str = r#"{
  "question": "Which of the following sorting algorithms are stable? (Select all that apply)",
  "options": [
    { "id": "a", "text": "Merge Sort" },
    { "id": "b", "text": "Quick Sort" },
    { "id": "c", "text": "Bubble Sort" },
    { "id": "d", "text": "Selection Sort" },
    { "id": "e", "text": "Insertion Sort" }
  ],
  "correct_option_indices": [0, 2, 4],
  "explanation": "Merge Sort, Bubble Sort, and Insertion Sort are stable — they preserve the relative order of elements with equal keys. Quick Sort and Selection Sort are not stable by default."
}"#;

const MCQ_MULTI_ML: &str = r#"{
  "question": "Which of the following are supervised learning algorithms? (Select all that apply)",
  "options": [
    { "id": "a", "text": "Linear Regression" },
    { "id": "b", "text": "K-Means Clustering" },
    { "id": "c", "text": "Random Forest" },
    { "id": "d", "text": "DBSCAN" },
    { "id": "e", "text": "Logistic Regression" }
  ],
  "correct_option_indices": [0, 2, 4],
  "explanation": "Linear Regression, Random Forest, and Logistic Regression are supervised (they learn from labeled data). K-Means and DBSCAN are unsupervised clustering algorithms."
}"#;

const MCQ_MULTI_CRYPTO: &str = r#"{
  "question": "Which properties must a cryptographic hash function satisfy? (Select all that apply)",
  "options": [
    { "id": "a", "text": "Preimage resistance" },
    { "id": "b", "text": "Second preimage resistance" },
    { "id": "c", "text": "Collision resistance" },
    { "id": "d", "text": "Reversibility" },
    { "id": "e", "text": "Fixed output size" }
  ],
  "correct_option_indices": [0, 1, 2, 4],
  "explanation": "Cryptographic hash functions must be preimage resistant, second-preimage resistant, collision resistant, and produce fixed-size output. They must NOT be reversible — that's the whole point."
}"#;

// ---------------------------------------------------------------------------
// Subjective MCQ content (McqContent JSON — no single correct answer)
// ---------------------------------------------------------------------------

const MCQ_SUBJ_WEB: &str = r#"{
  "question": "You're building a complex dashboard application with deeply nested state. Which Vue state management approach would you recommend, and why?",
  "options": [
    { "id": "a", "text": "Provide/inject for all shared state" },
    { "id": "b", "text": "Pinia store with modular stores per feature" },
    { "id": "c", "text": "Props drilling through every component" },
    { "id": "d", "text": "Global reactive object (reactive({}))" }
  ],
  "context": "Consider factors like developer experience, debugging, TypeScript support, and scalability. There is no single right answer — explain your reasoning."
}"#;

const MCQ_SUBJ_CRYPTO: &str = r#"{
  "question": "Your team needs to implement encryption for a messaging app. Which approach would you choose?",
  "options": [
    { "id": "a", "text": "RSA for all messages" },
    { "id": "b", "text": "AES-GCM with RSA key exchange" },
    { "id": "c", "text": "ChaCha20-Poly1305 with X25519 key exchange" },
    { "id": "d", "text": "Double Ratchet protocol (Signal Protocol)" }
  ],
  "context": "Consider: mobile performance, forward secrecy, message size, implementation complexity, and security guarantees. Explain the trade-offs of your choice."
}"#;

const MCQ_SUBJ_UX: &str = r#"{
  "question": "A stakeholder requests a pixel-perfect high-fidelity prototype before any user research has been conducted. How do you handle this?",
  "options": [
    { "id": "a", "text": "Build the high-fidelity prototype as requested" },
    { "id": "b", "text": "Propose low-fi wireframes first, then iterate to high-fi" },
    { "id": "c", "text": "Conduct guerrilla user research before any prototyping" },
    { "id": "d", "text": "Build a medium-fidelity prototype as a compromise" }
  ],
  "context": "Consider the risks of premature commitment to design details, the importance of stakeholder relationships, time constraints, and the value of early user feedback."
}"#;

// ---------------------------------------------------------------------------
// Essay content (EssayContent JSON)
// ---------------------------------------------------------------------------

const ESSAY_WEB: &str = r#"{
  "question": "Design and describe the architecture of a Todo application built with Vue 3 and a Rust backend. Include the component structure, state management approach, API endpoints, and database schema.",
  "guidelines": "Your response should cover: 1) Vue component hierarchy (at least 3 components), 2) State management strategy, 3) At least 4 REST API endpoints with methods and paths, 4) SQLite table schema for todos. Include code snippets where helpful.",
  "min_words": 200,
  "max_words": 1000,
  "rubric_criteria": [
    "Component architecture is well-structured and follows Vue 3 best practices",
    "API design follows RESTful conventions with appropriate HTTP methods",
    "Database schema is properly normalized with appropriate types",
    "State management approach is justified and appropriate for the complexity"
  ]
}"#;

const ESSAY_ML: &str = r#"{
  "question": "Design a neural network architecture for classifying handwritten digits (MNIST dataset). Describe the layer structure, activation functions, loss function, optimizer, and training strategy. Explain your choices.",
  "guidelines": "Include: 1) Input dimensions and preprocessing, 2) At least 2 hidden layers with justification for neuron counts, 3) Output layer design for 10 classes, 4) Choice of activation functions per layer, 5) Training hyperparameters (learning rate, batch size, epochs). Discuss potential overfitting strategies.",
  "min_words": 200,
  "max_words": 800,
  "rubric_criteria": [
    "Network architecture is appropriate for the image classification task",
    "Activation function choices are justified for each layer",
    "Training strategy includes reasonable hyperparameters",
    "Overfitting prevention techniques are discussed (dropout, regularization, etc.)"
  ]
}"#;

const ESSAY_UX: &str = r#"{
  "question": "You are redesigning the onboarding experience for a mobile banking app. Write a research plan that covers user interviews, personas, and a testing strategy for your proposed design.",
  "guidelines": "Include: 1) Research objectives (what you want to learn), 2) Participant recruitment criteria, 3) At least 5 interview questions, 4) A persona template based on expected user segments, 5) Usability testing plan with success metrics.",
  "min_words": 250,
  "max_words": 1000,
  "rubric_criteria": [
    "Research objectives are clearly defined and actionable",
    "Interview questions are open-ended and avoid leading bias",
    "Persona includes demographics, goals, frustrations, and context",
    "Testing plan includes measurable success criteria"
  ]
}"#;
