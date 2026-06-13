import js from "@eslint/js";

export default [
  {
    ignores: ["static/vendor/**", "e2e/**", "node_modules/**"],
  },
  {
    ...js.configs.recommended,
    files: ["static/**/*.js"],
    rules: {
      ...js.configs.recommended.rules,
      "no-var": "error",
      eqeqeq: "warn",
      "no-unused-vars": ["warn", { vars: "local" }],
    },
    languageOptions: {
      sourceType: "script",
      globals: {
        Alpine: "readonly",
        htmx: "readonly",
        document: "readonly",
        window: "readonly",
        localStorage: "readonly",
        history: "readonly",
        fetch: "readonly",
        console: "readonly",
        setTimeout: "readonly",
        clearTimeout: "readonly",
        setInterval: "readonly",
        clearInterval: "readonly",
        encodeURIComponent: "readonly",
        EventSource: "readonly",
        URLSearchParams: "readonly",
      },
    },
  },
];
