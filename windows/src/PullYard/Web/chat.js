(function () {
  "use strict";
  var host = window.chrome && window.chrome.webview;
  var items = [];
  var byId = {};
  var nodes = {};
  var openState = {};
  var expandedUser = {};
  var answers = {};
  var feedback = {};
  var meta = { busy: false, running: false, sleeping: false, pendingCount: 0, folder: "" };
  var stick = true;
  var tickTimer = null;

  function send(msg) {
    if (host) host.postMessage(msg);
  }

  function esc(s) {
    return String(s == null ? "" : s).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }

  if (window.marked) {
    marked.use({
      gfm: true,
      breaks: false,
      renderer: {
        html: function (token) {
          return esc(typeof token === "object" ? (token.text || token.raw || "") : token);
        }
      }
    });
  }

  function md(text) {
    if (!window.marked) return "<p>" + esc(text) + "</p>";
    try {
      return marked.parse(text || "");
    } catch (e) {
      return "<p>" + esc(text) + "</p>";
    }
  }

  function el(tag, cls, html) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (html != null) e.innerHTML = html;
    return e;
  }

  function button(label, cls, onClick) {
    var b = el("button", "b " + (cls || ""));
    b.textContent = label;
    b.addEventListener("click", function (e) {
      e.stopPropagation();
      onClick(e);
    });
    return b;
  }

  function atBottom() {
    return window.innerHeight + window.scrollY >= document.body.scrollHeight - 40;
  }

  function scrollDown() {
    window.scrollTo(0, document.body.scrollHeight);
    stick = true;
  }

  function anchor() {
    var kids = document.getElementById("list").children;
    for (var i = 0; i < kids.length; i++) {
      var r = kids[i].getBoundingClientRect();
      if (r.bottom > 0) return { id: kids[i].getAttribute("data-id"), top: r.top };
    }
    return null;
  }

  function restore(a) {
    if (!a) return;
    var n = nodes[a.id];
    if (!n || !n.parentNode) return;
    var delta = n.getBoundingClientRect().top - a.top;
    if (delta) window.scrollBy(0, delta);
  }

  function preserving(change) {
    var follow = stick;
    var a = follow ? null : anchor();
    change();
    if (follow) scrollDown();
    else restore(a);
  }

  function repin() {
    if (stick && window.innerHeight > 0) window.scrollTo(0, document.body.scrollHeight);
  }

  window.addEventListener("scroll", function () {
    if (window.innerHeight > 0) stick = atBottom();
  }, { passive: true });

  window.addEventListener("resize", repin);

  document.addEventListener("load", function (e) {
    if (e.target instanceof HTMLImageElement) repin();
  }, true);

  function relative(path) {
    var f = (meta.folder || "").replace(/[\\/]+$/, "");
    if (f && (path.indexOf(f + "\\") === 0 || path.indexOf(f + "/") === 0)) return path.slice(f.length + 1);
    return path;
  }

  function diffLines(item) {
    var out = [];
    var patch = item.structured && item.structured.structuredPatch;
    if (patch && patch.length) {
      patch.forEach(function (h) {
        var o = h.oldStart || 1, n = h.newStart || 1;
        (h.lines || []).forEach(function (l) {
          var c = l.charAt(0), body = l.slice(1);
          if (c === "+") { out.push({ k: "a", t: body, n: n }); n++; }
          else if (c === "-") { out.push({ k: "d", t: body, o: o }); o++; }
          else { out.push({ k: "", t: body, o: o, n: n }); o++; n++; }
        });
      });
      return out;
    }
    var input = item.input || {};
    if (item.name === "Write") {
      String(input.content || "").split("\n").forEach(function (l, i) { out.push({ k: "a", t: l, n: i + 1 }); });
      return out;
    }
    var edits = item.name === "MultiEdit" ? (input.edits || []) : [input];
    edits.forEach(function (e) {
      String(e.old_string || "").split("\n").forEach(function (l) { out.push({ k: "d", t: l }); });
      String(e.new_string || "").split("\n").forEach(function (l) { out.push({ k: "a", t: l }); });
    });
    return out;
  }

  function diffStat(item) {
    var patch = item.structured && item.structured.structuredPatch;
    if (patch && patch.length) {
      var a = 0, d = 0;
      patch.forEach(function (h) { (h.lines || []).forEach(function (l) { if (l.charAt(0) === "+") a++; else if (l.charAt(0) === "-") d++; }); });
      return { a: a, d: d };
    }
    if (item.name === "Write" && item.input && typeof item.input.content === "string") return { a: item.input.content.split("\n").length, d: 0 };
    return null;
  }

  function renderDiff(item, container) {
    var lines = diffLines(item);
    var box = el("div", "diff");
    var showAll = !!openState["all:" + item.id];
    var shown = showAll ? lines : lines.slice(0, 60);
    var html = "";
    shown.forEach(function (l) {
      var g = l.k === "d" ? (l.o || "") : (l.n || "");
      var sign = l.k === "a" ? "+" : l.k === "d" ? "-" : " ";
      html += '<div class="' + l.k + '"><span class="g">' + g + "</span>" + esc(sign + l.t) + "</div>";
    });
    box.innerHTML = html;
    container.appendChild(box);
    if (lines.length > 60 && !showAll) {
      container.appendChild(button("Show " + (lines.length - 60) + " more lines", "small", function () {
        openState["all:" + item.id] = true;
        update(item);
      }));
    }
  }

  function output(container, text, title) {
    if (!text || !String(text).trim()) return;
    if (title) container.appendChild(el("div", "lbl", esc(title)));
    var pre = el("pre", "out");
    pre.textContent = String(text).slice(0, 20000);
    container.appendChild(pre);
  }

  function renderDetails(item, det) {
    var input = item.input || {};
    switch (item.name) {
      case "Edit":
      case "MultiEdit":
      case "Write":
        if (input.file_path) {
          var row = el("div", "pathrow");
          row.appendChild(el("span", "", esc(relative(input.file_path))));
          row.appendChild(button("Open", "small", function () { send({ type: "open", path: input.file_path }); }));
          det.appendChild(row);
        }
        renderDiff(item, det);
        break;
      case "Bash":
        if (input.command) {
          var c = el("div", "cmd");
          c.textContent = "$ " + input.command;
          det.appendChild(c);
        }
        var s = item.structured || {};
        var out = (s.stdout || "") + (s.stderr ? "\n" + s.stderr : "");
        output(det, out || item.result);
        break;
      case "TodoWrite":
        (input.todos || []).forEach(function (t) {
          var st = t.status || "pending";
          var r = el("div", "todo " + st);
          r.appendChild(el("span", "i", st === "completed" ? "✔" : st === "in_progress" ? "◉" : "○"));
          var tt = el("span", "t");
          tt.textContent = t.content || t.subject || "";
          r.appendChild(tt);
          det.appendChild(r);
        });
        break;
      case "ExitPlanMode":
        det.appendChild(el("div", "md", md(input.plan || "")));
        break;
      case "AskUserQuestion":
        (input.questions || []).forEach(function (q) {
          var qq = el("div", "");
          qq.style.fontWeight = "600";
          qq.textContent = q.question || "";
          det.appendChild(qq);
        });
        if (item.result) output(det, item.result);
        break;
      case "Read":
        output(det, (item.structured && item.structured.fileContent) || item.result);
        break;
      default:
        if (input && Object.keys(input).length) output(det, JSON.stringify(input, null, 2), "Input");
        output(det, item.result, "Result");
    }
  }

  function permissionCard(item) {
    var p = item.permission;
    var card = el("div", "perm");
    var key = p.id;
    if (p.toolName === "AskUserQuestion") {
      var qs = (p.input && p.input.questions) || [];
      answers[key] = answers[key] || {};
      qs.forEach(function (q) {
        var text = q.question || "";
        var wrap = el("div", "");
        var title = el("div", "");
        title.style.fontWeight = "600";
        title.textContent = text;
        wrap.appendChild(title);
        (q.options || []).forEach(function (o) {
          var label = o.label || "";
          var opt = el("div", "opt" + (answers[key][text] === label ? " sel" : ""));
          opt.appendChild(el("span", "dot"));
          var body = el("div", "");
          var l = el("div", "");
          l.textContent = label;
          body.appendChild(l);
          if (o.description) {
            var d = el("div", "od");
            d.textContent = o.description;
            body.appendChild(d);
          }
          opt.appendChild(body);
          opt.addEventListener("click", function () {
            answers[key][text] = label;
            update(item);
          });
          wrap.appendChild(opt);
        });
        var other = el("input");
        other.type = "text";
        other.placeholder = "Other…";
        var isOption = (q.options || []).some(function (o) { return o.label === answers[key][text]; });
        other.value = isOption ? "" : (answers[key][text] || "");
        other.addEventListener("input", function () { answers[key][text] = other.value; });
        other.addEventListener("change", function () { update(item); });
        wrap.appendChild(other);
        card.appendChild(wrap);
      });
      var row = el("div", "row");
      var submit = button("Submit", "primary", function () {
        send({ type: "perm", id: key, action: "answer", answers: answers[key] });
      });
      submit.disabled = Object.keys(answers[key]).filter(function (k) { return answers[key][k]; }).length < qs.length;
      row.appendChild(submit);
      row.appendChild(button("Skip", "", function () {
        send({ type: "perm", id: key, action: "deny", message: "The user skipped the question" });
      }));
      card.appendChild(row);
      return card;
    }
    if (p.toolName === "ExitPlanMode") {
      card.appendChild(el("div", "", "<b>Claude has a plan. Start implementing?</b>"));
      var r1 = el("div", "row");
      r1.appendChild(button("Yes, auto-accept edits", "primary", function () { send({ type: "perm", id: key, action: "allow", mode: "acceptEdits" }); }));
      r1.appendChild(button("Yes, ask before edits", "", function () { send({ type: "perm", id: key, action: "allow", mode: "default" }); }));
      card.appendChild(r1);
      card.appendChild(denyRow(key, "What to change in the plan…"));
      return card;
    }
    card.appendChild(el("div", "q", esc(p.question)));
    if (p.description) card.appendChild(el("div", "desc", esc(p.description)));
    if (p.blockedPath) card.appendChild(el("div", "pathrow", esc(p.blockedPath)));
    var allowRow = el("div", "row");
    allowRow.appendChild(button("Allow", "primary", function () { send({ type: "perm", id: key, action: "allow" }); }));
    card.appendChild(allowRow);
    (p.suggestions || []).forEach(function (label, i) {
      var a = el("button", "always");
      a.textContent = "Allow, and don't ask again for " + label;
      a.addEventListener("click", function (e) {
        e.stopPropagation();
        send({ type: "perm", id: key, action: "always", index: i });
      });
      card.appendChild(a);
    });
    card.appendChild(denyRow(key, "No — tell Claude what to do instead…"));
    return card;
  }

  function denyRow(key, placeholder) {
    var row = el("div", "row");
    var input = el("input");
    input.type = "text";
    input.placeholder = placeholder;
    input.value = feedback[key] || "";
    input.addEventListener("input", function () { feedback[key] = input.value; });
    input.addEventListener("keydown", function (e) {
      if (e.key === "Enter") send({ type: "perm", id: key, action: "deny", message: input.value });
    });
    row.appendChild(input);
    row.appendChild(button("Deny", "", function () { send({ type: "perm", id: key, action: "deny", message: input.value }); }));
    return row;
  }

  function defaultOpen(item) {
    if (item.permission && !item.permission.resolved) return true;
    return ["Edit", "MultiEdit", "Write", "ExitPlanMode", "TodoWrite"].indexOf(item.name) >= 0;
  }

  function statusIcon(item) {
    if (item.status === "running") {
      if (item.permission && !item.permission.resolved) return '<span class="st wait">✋</span>';
      return '<span class="st"><span class="spin"></span></span>';
    }
    if (item.status === "done") return '<span class="st done">✔</span>';
    if (item.status === "error") return '<span class="st error">✖</span>';
    return '<span class="st denied">⊘</span>';
  }

  function build(item) {
    var node;
    switch (item.kind) {
      case "user": {
        var long = (item.text || "").split("\n").length > 12;
        node = el("div", "card user" + (long && !expandedUser[item.id] ? " clamped" : ""));
        if (item.images && item.images.length) {
          var imgs = el("div", "imgs");
          item.images.forEach(function (src) {
            var i = el("img");
            i.src = src;
            imgs.appendChild(i);
          });
          node.appendChild(imgs);
        }
        if (item.text) {
          var t = el("div", "txt");
          t.textContent = item.text;
          node.appendChild(t);
          if (long) {
            node.appendChild(button(expandedUser[item.id] ? "Collapse" : "Show all", "more", function () {
              expandedUser[item.id] = !expandedUser[item.id];
              update(item);
            }));
          }
        }
        break;
      }
      case "text":
        node = el("div", "md", item.text ? md(item.text) : "");
        if (!item.text) node.style.display = "none";
        break;
      case "thinking": {
        node = el("details", "thinking");
        if (openState[item.id]) node.open = true;
        node.appendChild(el("summary", "", "▸ 🧠 Thinking"));
        var body = el("div", "body");
        body.textContent = item.text || "";
        node.appendChild(body);
        node.addEventListener("toggle", function () { openState[item.id] = node.open; });
        if (!item.text) node.style.display = "none";
        break;
      }
      case "notice":
        node = el("div", "notice" + (item.error ? " err" : ""));
        node.textContent = item.text;
        break;
      case "tool": {
        var open = openState[item.id] != null ? openState[item.id] : defaultOpen(item);
        node = el("div", "card tool " + item.status + (open ? " open" : ""));
        var head = el("div", "th");
        var stat = diffStat(item);
        head.innerHTML = statusIcon(item) + "<span>" + esc(item.icon) + '</span><span class="name">' + esc(item.title) + '</span><span class="sum">' + esc(item.summary) + "</span>" +
          (stat ? '<span class="stat"><span class="plus">+' + stat.a + '</span> <span class="minus">−' + stat.d + "</span></span>" : "") + '<span class="chev">▾</span>';
        head.addEventListener("click", function () {
          openState[item.id] = !(openState[item.id] != null ? openState[item.id] : defaultOpen(item));
          update(item);
        });
        node.appendChild(head);
        var det = el("div", "det");
        if (open) renderDetails(item, det);
        node.appendChild(det);
        if (item.permission && !item.permission.resolved) node.appendChild(permissionCard(item));
        break;
      }
      default:
        node = el("div", "");
    }
    node.setAttribute("data-id", item.id);
    return node;
  }

  function replace(item) {
    var old = nodes[item.id];
    var next = build(item);
    nodes[item.id] = next;
    if (old && old.parentNode) old.parentNode.replaceChild(next, old);
  }

  function update(item) {
    var a = anchor();
    replace(item);
    restore(a);
  }

  function apply(list) {
    var container = document.getElementById("list");
    list.forEach(function (item) {
      if (byId[item.id]) {
        var idx = items.indexOf(byId[item.id]);
        if (idx >= 0) items[idx] = item;
        byId[item.id] = item;
        replace(item);
      } else {
        items.push(item);
        byId[item.id] = item;
        var n = build(item);
        nodes[item.id] = n;
        container.appendChild(n);
      }
    });
    renderTail();
  }

  function upsert(list) {
    preserving(function () { apply(list); });
  }

  function settle() {
    if (stick) scrollDown();
  }

  function reset(list) {
    items = [];
    byId = {};
    nodes = {};
    document.getElementById("list").innerHTML = "";
    apply(list);
    scrollDown();
    setTimeout(settle, 30);
    setTimeout(settle, 300);
  }

  function renderTail() {
    var tail = document.getElementById("tail");
    tail.innerHTML = "";
    if (!items.length && !meta.lastError) {
      var w = el("div", "welcome");
      w.appendChild(el("h3", "", "✨ Claude Code"));
      w.appendChild(el("div", "", "Describe a task — Claude will read the code, propose changes and ask before acting."));
      var sugs = el("div", "sugs");
      ["Explain the project structure", "Find bugs in the latest changes", "Write tests for the changed code"].forEach(function (s) {
        sugs.appendChild(button(s, "small", function () { send({ type: "send", text: s }); }));
      });
      w.appendChild(sugs);
      w.appendChild(el("div", "hint", "/ commands · @ files · Shift+Enter new line · paste a screenshot Ctrl+V"));
      tail.appendChild(w);
    }
    if (meta.busy) {
      var wk = el("div", "working");
      wk.innerHTML = '<span class="spin"></span><span>' + esc(meta.status || (meta.pendingCount > 0 ? "Waiting for you" : "Working…")) + '</span><span class="secs" id="secs"></span><span class="sp"></span>';
      wk.appendChild(button("Stop", "small", function () { send({ type: "stop" }); }));
      tail.appendChild(wk);
      tick();
      if (!tickTimer) tickTimer = setInterval(tick, 1000);
    } else if (tickTimer) {
      clearInterval(tickTimer);
      tickTimer = null;
    }
    if (meta.lastError) {
      var b = el("div", "banner");
      var m = el("div", "msg");
      m.textContent = "⚠ " + meta.lastError;
      b.appendChild(m);
      var row = el("div", "row");
      row.style.display = "flex";
      row.style.gap = "8px";
      row.appendChild(button("Restart session", "small", function () { send({ type: "restart" }); }));
      var s = meta.lastError.toLowerCase();
      if (["login", "log in", "logged in", "auth", "api key", "credential", "401", "oauth", "token"].some(function (k) { return s.indexOf(k) >= 0; })) {
        row.appendChild(button("Check Claude sign-in", "small coral", function () { send({ type: "checkAuth" }); }));
      }
      b.appendChild(row);
      tail.appendChild(b);
    }
  }

  function tick() {
    var secs = document.getElementById("secs");
    if (secs && meta.turnStartedAt) secs.textContent = Math.max(0, Math.floor((Date.now() - meta.turnStartedAt) / 1000)) + "s";
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
    if (!msg) return;
    switch (msg.op) {
      case "vars":
        document.getElementById("vars").textContent = msg.css;
        break;
      case "reset":
        if (msg.meta) meta = msg.meta;
        reset(msg.items || []);
        break;
      case "upsert":
        if (msg.meta) meta = msg.meta;
        upsert(msg.items || []);
        break;
      case "meta":
        meta = msg.meta;
        preserving(renderTail);
        break;
      case "bottom":
        scrollDown();
        break;
    }
  }

  document.addEventListener("click", function (e) {
    var a = e.target instanceof Element ? e.target.closest("a[href]") : null;
    if (!a) return;
    e.preventDefault();
    var href = a.getAttribute("href") || "";
    if (href.charAt(0) === "#") return;
    send({ type: "link", id: href });
  }, true);

  document.addEventListener("keydown", function (e) {
    if (e.key === "Escape" && meta.busy) send({ type: "stop" });
  });

  if (host) host.addEventListener("message", onMessage);
  document.addEventListener("DOMContentLoaded", function () {
    renderTail();
    send({ type: "ready" });
  });
})();
