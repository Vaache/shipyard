(function () {
  "use strict";
  var host = window.chrome && window.chrome.webview;
  var pendingScroll = null;

  function send(msg) {
    if (host) host.postMessage(msg);
  }

  window.post = function (type, id) {
    send({ type: type, id: String(id) });
  };

  function fileOf(el) {
    var s = el && el.closest ? el.closest("section.file") : null;
    return s ? s.getAttribute("data-path") : null;
  }

  window.postFile = function (el, type) {
    var p = fileOf(el);
    if (p !== null) send({ type: type, id: p });
  };

  window.toggleFile = function (el) {
    var s = el.closest("section.file");
    if (!s) return;
    var collapsed = !s.classList.contains("collapsed");
    s.classList.toggle("collapsed", collapsed);
    var chev = s.querySelector(".chev");
    if (chev) chev.textContent = collapsed ? "▸" : "▾";
    var body = s.querySelector(".fbody");
    if (body) body.style.display = collapsed ? "none" : "";
    send({ type: collapsed ? "collapse" : "expand", id: s.getAttribute("data-path") });
  };

  var handlerRx = /^\s*(post|postFile|toggleFile)\((.*)\)\s*;?\s*$/;
  var argRx = /'((?:[^'\\]|\\.)*)'/g;

  function decodeArg(s) {
    var t = document.createElement("textarea");
    t.innerHTML = s;
    return t.value;
  }

  function runInline(el, attr) {
    var code = el.getAttribute(attr);
    if (!code) return false;
    var m = handlerRx.exec(code);
    if (!m) return false;
    var args = [];
    var a;
    argRx.lastIndex = 0;
    while ((a = argRx.exec(m[2])) !== null) args.push(decodeArg(a[1]));
    if (m[1] === "post") window.post(args[0], args[1] || "");
    else if (m[1] === "postFile") window.postFile(el, args[0]);
    else if (m[1] === "toggleFile") window.toggleFile(el);
    return true;
  }

  document.addEventListener("click", function (e) {
    var t = e.target;
    if (!(t instanceof Element)) return;
    var gutter = t.closest("td[data-c]");
    if (gutter) {
      send({ type: "comment", id: gutter.getAttribute("data-c") });
      return;
    }
    var inline = t.closest("[onclick]");
    if (inline && runInline(inline, "onclick")) {
      e.preventDefault();
      return;
    }
    var a = t.closest("a[href]");
    if (a) {
      var href = a.getAttribute("href");
      if (!href || href.charAt(0) === "#") {
        if (href && href.length > 1) {
          var target = document.getElementById(href.slice(1)) || document.querySelector("[name='" + href.slice(1).replace(/'/g, "") + "']");
          if (target) target.scrollIntoView({ block: "start" });
        }
        e.preventDefault();
        return;
      }
      e.preventDefault();
      var url;
      try {
        url = new URL(href, "https://github.com/").href;
      } catch (err) {
        return;
      }
      send({ type: "link", id: url });
    }
  }, true);

  document.addEventListener("change", function (e) {
    var t = e.target;
    if (t instanceof Element && t.hasAttribute("onchange")) runInline(t, "onchange");
  }, true);

  document.addEventListener("keydown", function (e) {
    if ((e.ctrlKey || e.metaKey) && (e.key === "r" || e.key === "R")) {
      e.preventDefault();
      send({ type: "refresh", id: "" });
    }
    if (e.key === "F5") {
      e.preventDefault();
      send({ type: "refresh", id: "" });
    }
  });

  function scrollToFile(path) {
    var sections = document.querySelectorAll("section.file");
    for (var i = 0; i < sections.length; i++) {
      if (sections[i].getAttribute("data-path") === path) {
        sections[i].scrollIntoView({ block: "start" });
        return true;
      }
    }
    return false;
  }

  function render(msg) {
    var root = document.getElementById("root");
    var y = window.scrollY;
    document.getElementById("dyn").textContent = msg.css || "";
    document.body.className = msg.bodyClass || "";
    root.innerHTML = msg.html || "";
    var collapsed = root.querySelectorAll("section.file.collapsed .fbody");
    for (var i = 0; i < collapsed.length; i++) collapsed[i].style.display = "none";
    if (msg.scroll === "bottom") {
      window.scrollTo(0, document.body.scrollHeight);
    } else if (msg.scroll === "top") {
      window.scrollTo(0, 0);
    } else if (msg.scroll && msg.scroll.indexOf("file:") === 0) {
      scrollToFile(msg.scroll.slice(5));
    } else {
      window.scrollTo(0, y);
    }
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
    if (!msg || !msg.op) return;
    switch (msg.op) {
      case "render":
        render(msg);
        break;
      case "fileBody": {
        var sections = document.querySelectorAll("section.file");
        for (var i = 0; i < sections.length; i++) {
          if (sections[i].getAttribute("data-path") === msg.path) {
            var body = sections[i].querySelector(".fbody");
            if (body) {
              body.innerHTML = msg.html;
              body.style.display = "";
            }
          }
        }
        break;
      }
      case "scrollToFile":
        scrollToFile(msg.path);
        break;
      case "scrollBottom":
        window.scrollTo(0, document.body.scrollHeight);
        break;
      case "find":
        if (window.find) window.find(msg.text || "");
        break;
    }
  }

  if (host) host.addEventListener("message", onMessage);
  document.addEventListener("DOMContentLoaded", function () {
    send({ type: "ready", id: "" });
  });
})();
