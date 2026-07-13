# Code Editor: C++ (graded)

Same architecture as `editor-javascript`, but the source is C/C++: it is
interpreted by the bundled **JSCPP** (a C/C++ interpreter in JavaScript) running
inside `boa-runner-core`'s Boa engine — both for in-browser live eval and the
host-side deterministic grader. Boa lacks the deprecated `String.prototype.substr`
that JSCPP uses, so the core polyfills it before loading JSCPP.

Intro subset only: variables, arrays, pointers, control flow, functions, and
basic `iostream`/`cmath`/`cstring`. No STL containers, templates, or namespaces.

See `../editor-javascript/README.md` for the shared architecture and content
schema. Build via `../editor-shared/build.sh cpp` (or `all`).
