// Self-contained CodeMirror 6 surface exposed as window.AlexCM6. One language
// per plugin: JS/TS pass javascript({ typescript }); C++ uses cpp(); Python uses python().
export { EditorState, Compartment } from "@codemirror/state";
export { EditorView, keymap, lineNumbers, highlightActiveLine,
         highlightActiveLineGutter, drawSelection, rectangularSelection,
         crosshairCursor, dropCursor } from "@codemirror/view";
export { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
export { indentOnInput, bracketMatching, foldGutter, foldKeymap,
         syntaxHighlighting, defaultHighlightStyle } from "@codemirror/language";
export { closeBrackets, closeBracketsKeymap, autocompletion,
         completionKeymap } from "@codemirror/autocomplete";
export { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
export { javascript } from "@codemirror/lang-javascript";
export { cpp } from "@codemirror/lang-cpp";
export { python } from "@codemirror/lang-python";
export { oneDark } from "@codemirror/theme-one-dark";
