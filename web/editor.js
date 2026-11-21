"use strict";
(() => {
  const bridge = window.chrome.webview;
  const form = document.querySelector("#preferences");
  const message = document.querySelector("#message");
  let editor;
  let session = 0;
  let ready = false;
  let preferences;
  let page = "";
  let uploads = 0;
  let composing = false;
  let pendingFlush = false;
  let lastValue = "";
  let baseline = "";
  let originalBody = "";
  const post = (type, data = {}) => bridge.postMessage({ type, session, ...data });
  const value = () => editor.getValue().replaceAll("https://notes.esticker.local/", "");
  const emit = () => {
    if (!ready || page !== "editor") return;
    const current = value();
    const body = current === baseline ? originalBody : current;
    if (body.length > 100000) {
      editor.tip("正文超过 100000 字符，请删减后再保存；窗口将保持打开。", 0);
      post("invalid", { message: "正文超过 100000 字符，尚未保存" });
      return;
    }
    if (body !== lastValue) { lastValue = body; post("change", { body }); }
  };
  const flush = () => {
    if (!ready || uploads || composing) { pendingFlush = true; return; }
    pendingFlush = false;
    if (page === "editor") {
      emit();
      if (value().length > 100000) return;
    } else if (page === "settings") emitSettings();
    post("flushed");
  };
  const emitSettings = () => {
    for (const input of form.elements) {
      if (!input.name) continue;
      const target = Object.hasOwn(preferences.editor, input.name) ? preferences.editor : preferences;
      target[input.name] = input.type === "checkbox" ? input.checked : input.tagName === "SELECT" && !["mode", "font", "theme", "code_theme"].includes(input.name) ? Number(input.value) : input.value;
    }
    post("preferences", { preferences });
  };
  const loadSettings = () => {
    for (const input of form.elements) {
      if (!input.name) continue;
      const target = Object.hasOwn(preferences.editor, input.name) ? preferences.editor : preferences;
      if (input.type === "checkbox") input.checked = target[input.name];
      else {
        const text = String(target[input.name]);
        if (![...input.options].some(option => option.value === text)) input.add(new Option(text, text));
        input.value = text;
      }
    }
  };
  const upload = async files => {
    const currentSession = session;
    for (const file of files) {
      if (file.size > 20 * 1024 * 1024) return "图片不能超过 20 MiB";
      if (!/image\/(png|jpeg|gif|bmp)/.test(file.type)) return "仅支持 PNG、JPEG、GIF、BMP 图片";
    }
    uploads += files.length;
    for (const file of files) {
      try {
        const data = await new Promise((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => resolve(reader.result.split(",")[1]);
          reader.onerror = reject;
          reader.readAsDataURL(file);
        });
        bridge.postMessage({ type: "image", session: currentSession, data });
      } catch {
        uploads--;
        editor.tip("未能读取图片，请重试");
        if (pendingFlush) flush();
      }
    }
    return null;
  };
  bridge.addEventListener("message", event => {
    const data = event.data;
    if (data.type === "open") {
      ready = false;
      session = data.session;
      page = data.page;
      preferences = data.preferences;
      pendingFlush = false;
      uploads = 0;
      editor?.destroy();
      editor = null;
      document.querySelector("#settings").hidden = page !== "settings";
      document.querySelector("#editor").hidden = page !== "editor";
      message.hidden = true;
      if (page === "settings") {
        document.querySelector("#paper-options").replaceChildren(...data.papers.map((name, index) => new Option(name, String(index))));
        loadSettings();
        ready = true;
        post("loaded");
        return;
      }
      const settings = preferences.editor;
      const fonts = { sans: '"Microsoft YaHei UI", sans-serif', serif: 'SimSun, serif', mono: 'Consolas, "Microsoft YaHei UI", monospace' };
      document.documentElement.style.setProperty("--editor-font", fonts[settings.font]);
      document.documentElement.style.setProperty("--editor-line-height", settings.line_height);
      document.documentElement.style.setProperty("--editor-font-size", settings.font_size + "px");
      lastValue = data.body;
      originalBody = data.body;
      editor = new Vditor("editor", {
        cdn: location.origin + "/vditor", lang: "zh_CN", mode: settings.mode,
        theme: settings.theme, height: "100%", value: data.body,
        cache: { enable: false }, placeholder: "从这里开始，写下一张便签装不下的想法…",
        counter: { enable: settings.counter, type: "markdown" },
        outline: { enable: settings.outline, position: "left" },
        toolbarConfig: { pin: true, hide: false },
        toolbar: ["headings", "bold", "italic", "strike", "|", "list", "ordered-list", "check", "outdent", "indent", "quote", "|", "code", "inline-code", "link", "upload", "table", "line", "|", "undo", "redo", "|", "edit-mode", "both", "outline", "export"],
        preview: {
          delay: 150, maxWidth: settings.content_width, actions: [],
          theme: { current: settings.theme === "dark" ? "dark" : "light", path: location.origin + "/vditor/dist/css/content-theme" },
          markdown: { sanitize: true, linkBase: "https://notes.esticker.local/" },
          hljs: { enable: true, style: settings.code_theme },
          math: { engine: "KaTeX" }
        },
        upload: { handler: upload, accept: "image/png,image/jpeg,image/gif,image/bmp", max: 20 * 1024 * 1024 },
        input: emit,
        after: () => {
          ready = true;
          baseline = value();
          document.querySelectorAll(".vditor-reset").forEach(element => element.style.fontSize = settings.font_size + "px");
          post("loaded");
          if (pendingFlush) flush();
          editor.focus();
        }
      });
    } else if (data.session === session && data.type === "flush") flush();
    else if (data.session === session && data.type === "insert") {
      editor.insertValue(data.text);
      emit();
    } else if (data.session === session && data.type === "image-result") {
      uploads = Math.max(0, uploads - 1);
      if (data.reference) { editor.insertValue(`\n![图片](${data.reference})\n`); emit(); }
      else editor.tip(data.error || "图片导入失败");
      if (pendingFlush) flush();
    }
  });
  form.addEventListener("change", emitSettings);
  document.querySelector("#reset").addEventListener("click", () => { post("reset"); });
  document.addEventListener("compositionstart", () => composing = true);
  document.addEventListener("compositionend", () => { composing = false; setTimeout(() => { emit(); if (pendingFlush) flush(); }, 0); });
  document.addEventListener("keydown", event => {
    if (!preferences) return;
    if (!preferences.markdown_shortcuts && event.ctrlKey && ["b", "i", "k"].includes(event.key.toLowerCase())) {
      event.preventDefault(); event.stopImmediatePropagation(); return;
    }
    if ((event.ctrlKey && (event.key.toLowerCase() === "s" || event.key === "Enter")) || (event.key === "Escape" && !composing)) {
      event.preventDefault(); event.stopImmediatePropagation();
      post("action", { action: event.key === "Escape" ? "back" : "save" });
    }
  }, true);
  document.addEventListener("click", event => {
    const link = event.target.closest("a[href]");
    if (link) { event.preventDefault(); post("link", { url: link.href }); }
  });
  window.addEventListener("error", event => { message.hidden = false; message.textContent = "编辑器加载失败：" + event.message; post("error", { message: event.message }); });
  post("ready");
})();
