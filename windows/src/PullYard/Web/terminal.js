(function () {
  "use strict";
  var host = window.chrome && window.chrome.webview;
  var term = null;
  var fit = null;
  var lastSize = "";

  function send(msg) {
    if (host) host.postMessage(msg);
  }

  function b64ToBytes(b64) {
    var bin = atob(b64);
    var out = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }

  function reportSize() {
    if (!term || !fit) return;
    try {
      fit.fit();
    } catch (e) {
      return;
    }
    var size = term.cols + "x" + term.rows;
    if (size !== lastSize) {
      lastSize = size;
      send({ type: "resize", cols: term.cols, rows: term.rows });
    }
  }

  function applyTheme(t) {
    if (!term || !t) return;
    term.options.theme = {
      background: t.background,
      foreground: t.foreground,
      cursor: t.cursor,
      cursorAccent: t.background,
      selectionBackground: t.selection
    };
    document.body.style.background = t.background;
  }

  function init() {
    term = new Terminal({
      fontFamily: '"Cascadia Mono", "Cascadia Code", Consolas, monospace',
      fontSize: 13,
      lineHeight: 1.15,
      cursorBlink: true,
      allowProposedApi: true,
      scrollback: 10000,
      macOptionIsMeta: false,
      allowTransparency: false
    });
    fit = new FitAddon.FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon.WebLinksAddon(function (e, uri) {
      send({ type: "link", id: uri });
    }));
    term.open(document.getElementById("term"));
    term.onData(function (data) {
      send({ type: "input", data: data });
    });
    term.onBinary(function (data) {
      send({ type: "input", data: data });
    });
    term.attachCustomKeyEventHandler(function (e) {
      if (e.type === "keydown" && e.ctrlKey && e.shiftKey && (e.key === "C" || e.key === "c")) {
        var sel = term.getSelection();
        if (sel) send({ type: "copy", data: sel });
        return false;
      }
      if (e.type === "keydown" && e.ctrlKey && (e.key === "v" || e.key === "V")) {
        send({ type: "paste" });
        return false;
      }
      if (e.type === "keydown" && e.ctrlKey && (e.key === "c" || e.key === "C") && term.hasSelection()) {
        send({ type: "copy", data: term.getSelection() });
        term.clearSelection();
        return false;
      }
      return true;
    });
    window.addEventListener("resize", reportSize);
    new ResizeObserver(reportSize).observe(document.getElementById("term"));
    setTimeout(reportSize, 30);
    send({ type: "ready" });
  }

  function onMessage(e) {
    var msg = e.data;
    if (typeof msg === "string") {
      try {
        msg = JSON.parse(msg);
      } catch (err) {
        return;
      }
    }
    if (!msg || !term) return;
    switch (msg.op) {
      case "output":
        term.write(b64ToBytes(msg.data));
        break;
      case "theme":
        applyTheme(msg.theme);
        break;
      case "focus":
        term.focus();
        reportSize();
        break;
      case "reset":
        term.reset();
        break;
      case "paste":
        term.paste(msg.data || "");
        break;
      case "fontSize":
        term.options.fontSize = msg.size;
        reportSize();
        break;
    }
  }

  if (host) host.addEventListener("message", onMessage);
  document.addEventListener("DOMContentLoaded", init);
})();
