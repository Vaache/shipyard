(function () {
  var root = document.getElementById("demo");
  if (!root) return;

  var ICONS = {
    bell: '<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/>',
    eye: '<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>',
    user: '<circle cx="12" cy="8" r="4"/><path d="M4 21c0-4 4-6 8-6s8 2 8 6"/>',
    at: '<circle cx="12" cy="12" r="4"/><path d="M16 8v5a3 3 0 0 0 6 0v-1a10 10 0 1 0-4 8"/>',
    book: '<path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20V3H6.5A2.5 2.5 0 0 0 4 5.5z"/><path d="M4 19.5V21h16"/>',
    board: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16M15 4v16"/>',
    spark: '<path d="m12 3 1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z"/>',
    pr: '<circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5v7M18 15.5V10a3 3 0 0 0-3-3h-4"/><path d="m13 5-2 2 2 2"/>',
    merge: '<circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="12" r="2.5"/><path d="M6 8.5v7M6 8.5c0 3 3 3.5 9.5 3.5"/>',
    chat: '<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>',
    commit: '<circle cx="12" cy="12" r="3.5"/><path d="M3 12h5.5M15.5 12H21"/>',
    check: '<path d="M20 6 9 17l-5-5"/>',
    x: '<path d="M18 6 6 18M6 6l12 12"/>',
    clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
    file: '<path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><path d="M14 3v6h6"/>',
    folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
    back: '<path d="m15 18-6-6 6-6"/>',
    term: '<path d="m4 17 6-6-6-6M12 19h8"/>',
    edit: '<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/>',
    send: '<path d="M12 19V5M5 12l7-7 7 7"/>'
  };
  function icon(name, cls) {
    return '<svg class="' + (cls || "ic") + '" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' + ICONS[name] + "</svg>";
  }
  function esc(s) {
    return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }
  function avatar(name) {
    var hues = { mkato: "#34d399,#4aa8dc", jlee: "#e8b44c,#f472b6", sofia: "#9d8cf0,#5bb8ec", dvorak: "#ee6f5f,#f59e0b", bot: "#5f6b80,#94a3b8", arivera: "#f472b6,#818cf8" };
    return '<span class="d-av" style="background:' + (hues[name] || "#5f6b80,#94a3b8").split(",")[0] + '"></span>';
  }

  var PRS = {
    91: {
      repo: "acme/ios", title: "Haptics on pull to refresh", author: "dvorak", state: "open", ago: "now", add: 22, del: 3, base: "main", head: "feat/pull-haptics",
      body: "Plays a light haptic when the pull crosses the refresh threshold, once per gesture.",
      commits: [["Add haptic at refresh threshold", "dvorak", "now"]],
      checks: [["run", "build", "running"], ["ok", "lint", "18s"]],
      files: [{ path: "Sources/Shared/PullIndicator.swift", st: "M", add: 22, del: 3, lines: [
        ["h", "", "", "@@ -12,6 +12,9 @@ struct PullIndicator: View {"],
        ["", "12", "12", "    .onChange(of: armed) { _, armed in"],
        ["a", "", "13", "        guard armed else { return }"],
        ["a", "", "14", "        haptics.perform(.alignment, performanceTime: .now)"],
        ["", "13", "15", "    }"]
      ] }]
    },
    482: {
      repo: "acme/web-app", title: "Speed up search with debounced queries", author: "mkato", state: "open", ago: "2h", add: 48, del: 12, base: "main", head: "feat/search-debounce",
      body: "Debounces the search box by 250 ms so we stop firing a request on every keystroke. Adds a small <code>useDebounce</code> hook with tests.",
      rerequest: { who: "sofia", n: 2 },
      review: { who: "sofia", text: "Cancel the in-flight request when the query changes, otherwise stale results can win the race.", file: "src/search/useSearch.ts", line: 26 },
      commits: [["Add useDebounce hook", "mkato", "3h"], ["Debounce search queries", "mkato", "3h"], ["Show spinner while loading", "mkato", "2h"], ["Address lint", "mkato", "2h"]],
      checks: [["ok", "build", "1m 12s"], ["ok", "unit tests", "48s"], ["ok", "lint", "21s"], ["ok", "e2e / chrome", "3m 40s"]],
      files: [
        { path: "src/search/useSearch.ts", st: "M", add: 18, del: 6, lines: [
          ["h", "", "", "@@ -18,9 +18,15 @@ export function useSearch(query) {"],
          ["", "18", "18", "  const [results, setResults] = useState([])"],
          ["", "19", "19", "  const [loading, setLoading] = useState(false)"],
          ["d", "20", "", "  useEffect(() => {"],
          ["d", "21", "", "    fetchResults(query).then(setResults)"],
          ["d", "22", "", "  }, [query])"],
          ["a", "", "20", "  const debounced = useDebounce(query, 250)"],
          ["a", "", "21", ""],
          ["a", "", "22", "  useEffect(() => {"],
          ["a", "", "23", "    if (!debounced) return setResults([])"],
          ["a", "", "24", "    setLoading(true)"],
          ["a", "", "25", "    fetchResults(debounced)"],
          ["a", "", "26", "      .then(setResults)"],
          ["a", "", "27", "      .finally(() => setLoading(false))"],
          ["a", "", "28", "  }, [debounced])"],
          ["", "23", "29", ""],
          ["", "24", "30", "  return { results, loading }"]
        ] },
        { path: "src/hooks/useDebounce.ts", st: "A", add: 14, del: 0, lines: [
          ["h", "", "", "@@ -0,0 +1,14 @@"],
          ["a", "", "1", "import { useEffect, useState } from \"react\""],
          ["a", "", "2", ""],
          ["a", "", "3", "export function useDebounce<T>(value: T, ms: number) {"],
          ["a", "", "4", "  const [debounced, setDebounced] = useState(value)"],
          ["a", "", "5", "  useEffect(() => {"],
          ["a", "", "6", "    const id = setTimeout(() => setDebounced(value), ms)"],
          ["a", "", "7", "    return () => clearTimeout(id)"],
          ["a", "", "8", "  }, [value, ms])"],
          ["a", "", "9", "  return debounced"],
          ["a", "", "10", "}"]
        ] },
        { path: "src/search/SearchBar.test.tsx", st: "M", add: 16, del: 6, lines: [
          ["h", "", "", "@@ -12,8 +12,18 @@ describe(\"SearchBar\", () => {"],
          ["d", "12", "", "  it(\"fetches on every keystroke\", async () => {"],
          ["a", "", "12", "  it(\"waits 250 ms before fetching\", async () => {"],
          ["", "13", "13", "    render(<SearchBar />)"],
          ["a", "", "14", "    vi.useFakeTimers()"],
          ["", "14", "15", "    await user.type(input, \"ship\")"],
          ["a", "", "16", "    expect(fetchResults).not.toHaveBeenCalled()"],
          ["a", "", "17", "    vi.advanceTimersByTime(250)"],
          ["", "15", "18", "    expect(fetchResults).toHaveBeenCalledTimes(1)"]
        ] }
      ]
    },
    479: {
      repo: "acme/web-app", title: "Add dark mode to the settings screen", author: "jlee", state: "open", ago: "5h", add: 214, del: 37, base: "main", head: "feat/dark-settings",
      body: "Moves settings colors to theme tokens and adds a light / dark / system picker.",
      commits: [["Theme tokens for settings", "jlee", "6h"], ["Appearance picker", "jlee", "5h"]],
      checks: [["ok", "build", "1m 05s"], ["run", "unit tests", "running"], ["ok", "lint", "19s"]],
      files: [{ path: "src/settings/Appearance.tsx", st: "A", add: 62, del: 0, lines: [
        ["h", "", "", "@@ -0,0 +1,62 @@"],
        ["a", "", "1", "export function AppearancePicker() {"],
        ["a", "", "2", "  const [mode, setMode] = useAppearance()"],
        ["a", "", "3", "  return <Segmented value={mode} onChange={setMode}"],
        ["a", "", "4", "    options={[\"light\", \"dark\", \"system\"]} />"],
        ["a", "", "5", "}"]
      ] }]
    },
    213: {
      repo: "acme/api", title: "Retry token refresh on 401", author: "sofia", state: "draft", ago: "yesterday", add: 92, del: 18, base: "main", head: "fix/refresh-retry",
      body: "Retries the original request once after a successful token refresh. Still needs tests for the concurrent case.",
      commits: [["Retry once after refresh", "sofia", "1d"]],
      checks: [["fail", "unit tests", "2 failed"], ["ok", "build", "58s"]],
      files: [{ path: "src/auth/client.ts", st: "M", add: 92, del: 18, lines: [
        ["h", "", "", "@@ -40,6 +40,14 @@ async function request(req) {"],
        ["", "40", "40", "  const res = await fetch(req)"],
        ["a", "", "41", "  if (res.status === 401 && !req.retried) {"],
        ["a", "", "42", "    await refreshToken()"],
        ["a", "", "43", "    return request({ ...req, retried: true })"],
        ["a", "", "44", "  }"],
        ["", "41", "45", "  return res"]
      ] }]
    },
    77: {
      repo: "acme/ios", title: "Fix crash when the list is empty", author: "arivera", state: "merged", ago: "2d", add: 9, del: 3, base: "main", head: "fix/empty-list",
      body: "Guards against an empty array before reading the first item.",
      commits: [["Guard empty list", "arivera", "2d"]],
      checks: [["ok", "build", "4m 02s"], ["ok", "tests", "2m 10s"]],
      files: [{ path: "App/Feed/FeedView.swift", st: "M", add: 9, del: 3, lines: [
        ["h", "", "", "@@ -31,7 +31,9 @@ struct FeedView: View {"],
        ["d", "31", "", "        let first = items[0]"],
        ["a", "", "31", "        guard let first = items.first else {"],
        ["a", "", "32", "            return AnyView(EmptyFeed())"],
        ["a", "", "33", "        }"]
      ] }]
    },
    58: {
      repo: "acme/infra", title: "Bump dependencies for Q4", author: "arivera", state: "open", ago: "1d", add: 140, del: 131, base: "main", head: "chore/deps-q4",
      body: "Routine dependency bumps. Node 22, Terraform providers and the CI images.",
      commits: [["Bump node to 22", "arivera", "1d"], ["Bump terraform providers", "arivera", "1d"]],
      checks: [["fail", "plan / staging", "failed"], ["ok", "lint", "12s"]],
      files: [{ path: ".github/workflows/ci.yml", st: "M", add: 2, del: 2, lines: [
        ["h", "", "", "@@ -9,7 +9,7 @@ jobs:"],
        ["d", "9", "", "      node-version: 20"],
        ["a", "", "9", "      node-version: 22"]
      ] }]
    }
  };

  var SECTIONS = [
    { h: "Inbox" },
    { id: "notifications", title: "Notifications", icon: "bell", color: "#e8b44c", count: 4 },
    { h: "Pull requests" },
    { id: "review", title: "Review requests", icon: "eye", color: "#5bb8ec", count: 3, list: [482, 479, 213] },
    { id: "mine", title: "Created by me", icon: "user", color: "#9d8cf0", count: 2, list: [77, 58] },
    { id: "mentioned", title: "Mentioned", icon: "at", color: "#818cf8", list: [213] },
    { h: "Code" },
    { id: "repos", title: "Repositories", icon: "book", color: "#2dd4bf" },
    { id: "projects", title: "Projects", icon: "board", color: "#6366f1" },
    { h: "AI" },
    { id: "claude", title: "Claude Code", icon: "spark", color: "#ee6f5f" }
  ];

  var NOTIFS = [
    { pr: 482, type: "pr", text: "sofia requested changes", reason: "review requested" },
    { pr: 58, type: "fail", text: "CI failed on plan / staging", reason: "ci activity" },
    { pr: 479, type: "pr", text: "jlee requested your review", reason: "review requested" },
    { pr: 77, type: "merge", text: "Merged into main", reason: "author" }
  ];

  var REPOS = [
    ["acme/web-app", "TypeScript", "#3178c6", "Customer web app", 128],
    ["acme/api", "Go", "#00add8", "Public REST API", 64],
    ["acme/ios", "Swift", "#f05138", "iOS client", 51],
    ["acme/infra", "HCL", "#844fba", "Terraform and CI", 12]
  ];

  var state = { section: "review", pr: 482, tab: "files", file: 0, filter: "open", repo: 0, chat: null, detailOpen: false, fresh: null, freshDone: false, updated: Date.now(), requested: false, rerqHidden: false };

  function stateColor(s) {
    return s === "merged" ? "var(--violet)" : s === "draft" ? "#94a3b8" : s === "closed" ? "var(--coral)" : "var(--green)";
  }
  function stateIcon(s) {
    return s === "merged" ? "merge" : "pr";
  }

  function sidebar() {
    var h = '<div class="d-side"><div class="d-tl"><i style="background:#ff5f57"></i><i style="background:#febc2e"></i><i style="background:#28c840"></i></div>';
    SECTIONS.forEach(function (s) {
      if (s.h) { h += '<div class="d-h">' + s.h + "</div>"; return; }
      var on = state.section === s.id ? " on" : "";
      var c = s.count ? '<span class="d-c' + (s.id === "review" ? " hot" : "") + '">' + s.count + "</span>" : "";
      h += '<button class="d-it' + on + '" data-sec="' + s.id + '" style="--c:' + s.color + '"><span class="d-tile" style="--c:' + s.color + '">' + icon(s.icon, "ti") + "</span><span>" + s.title + "</span>" + c + "</button>";
    });
    h += '<div class="d-me">' + avatar("arivera") + '<div><b>Alex Rivera</b><small>@arivera</small></div></div></div>';
    return h;
  }

  function sectionById(id) {
    for (var i = 0; i < SECTIONS.length; i++) if (SECTIONS[i].id === id) return SECTIONS[i];
  }

  function listColumn() {
    var s = sectionById(state.section);
    var h = '<div class="d-list"><div class="d-bar"><div><b>' + s.title + "</b><small>" + listCount() + "</small></div></div>";
    if (s.list) {
      h += '<div class="d-seg">' + ["open", "closed", "all"].map(function (f) {
        return '<button data-filter="' + f + '" class="' + (state.filter === f ? "on" : "") + '">' + f.charAt(0).toUpperCase() + f.slice(1) + "</button>";
      }).join("") + "</div>";
      var ids = s.list.filter(function (id) {
        var st = PRS[id].state;
        return state.filter === "all" || (state.filter === "open" ? st !== "merged" : st === "merged");
      });
      if (!ids.length) h += '<div class="d-empty">Nothing here</div>';
      ids.forEach(function (id) { h += prRow(id); });
      h += '<div class="d-upd">Updated <span data-ago>' + agoText() + "</span></div>";
    } else if (state.section === "notifications") {
      NOTIFS.forEach(function (n) {
        var p = PRS[n.pr];
        var col = n.type === "fail" ? "var(--coral)" : n.type === "merge" ? "var(--violet)" : "var(--violet)";
        h += '<button class="d-row' + (state.pr === n.pr && state.detail === "pr" ? " on" : "") + '" data-pr="' + n.pr + '"><span class="d-state" style="color:' + col + ';background:color-mix(in srgb,' + col + ' 16%,transparent)">' + icon(n.type === "fail" ? "x" : n.type === "merge" ? "merge" : "pr", "ti") + '</span><span class="d-rt"><b>' + esc(p.title) + "</b><small>" + p.repo + " #" + n.pr + " · " + n.text + '</small><small class="d-reason">' + n.reason + '</small></span><span class="d-dot"></span></button>';
      });
    } else if (state.section === "repos") {
      REPOS.forEach(function (r, i) {
        h += '<button class="d-row' + (state.detail === "repo" && state.repo === i ? " on" : "") + '" data-repo="' + i + '"><span class="d-tile sm" style="--c:#2dd4bf">' + icon("book", "ti") + '</span><span class="d-rt"><b>' + r[0] + "</b><small>" + r[3] + '</small><small><i class="d-lang" style="background:' + r[2] + '"></i>' + r[1] + " · ★ " + r[4] + "</small></span></button>";
      });
    } else if (state.section === "projects") {
      h += '<button class="d-row on"><span class="d-tile sm" style="--c:#6366f1">' + icon("board", "ti") + '</span><span class="d-rt"><b>Q4 roadmap</b><small>12 items · 3 columns</small></span></button>';
    } else if (state.section === "claude") {
      h += '<div class="d-sub">Open</div>';
      h += '<button class="d-row on"><span class="d-sdot" style="background:' + (state.chat && state.chat.waiting ? "#e8b44c" : "#5dbe8a") + '"></span><span class="d-rt"><b>Cancel stale searches</b><small>web-app · <i class="d-chip v">feat/search-debounce</i></small></span></button>';
      h += '<div class="d-sub">Projects</div>';
      [["web-app", "#5bb8ec", 6], ["api", "#9d8cf0", 12], ["ios", "#fb923c", 3]].forEach(function (p) {
        h += '<div class="d-proj"><span class="d-tile sm" style="--c:' + p[1] + '">' + icon("folder", "ti") + "</span><b>" + p[0] + "</b><small>" + p[2] + "</small></div>";
      });
    }
    return h + "</div>";
  }

  function listCount() {
    var s = sectionById(state.section);
    if (s.list) return s.list.length;
    if (state.section === "notifications") return NOTIFS.length;
    if (state.section === "repos") return REPOS.length;
    return "";
  }

  function prRow(id) {
    var p = PRS[id];
    var on = (state.detail === "pr" && state.pr === id ? " on" : "") + (state.fresh === id && !state.freshDone ? " d-new" : "");
    var col = stateColor(p.state);
    return '<button class="d-row' + on + '" data-pr="' + id + '"><span class="d-state" style="color:' + col + ";background:color-mix(in srgb," + col + ' 16%,transparent)">' + icon(stateIcon(p.state), "ti") + '</span><span class="d-rt"><b>' + esc(p.title) + "</b><small>" + p.repo + "#" + id + " · " + p.ago + "</small><small>" + avatar(p.author) + p.author + ' <i class="g">+' + p.add + '</i> <i class="r">−' + p.del + "</i></small></span></button>";
  }

  function detail() {
    var back = '<button class="d-back" data-back>' + icon("back", "ti") + "Back</button>";
    if (state.section === "claude") return '<div class="d-det">' + back + chatView() + "</div>";
    if (state.detail === "repo") return '<div class="d-det">' + back + repoView() + "</div>";
    if (state.section === "projects") return '<div class="d-det">' + back + boardView() + "</div>";
    if (state.detail !== "pr") return '<div class="d-det d-placeholder"><span class="d-tile lg" style="--c:var(--violet)">' + icon("pr", "ti") + "</span><p>Select a pull request</p></div>";
    var p = PRS[state.pr];
    var col = stateColor(p.state);
    var h = '<div class="d-det">' + back + '<div class="d-head"><h3>' + esc(p.title) + " <span>#" + state.pr + '</span></h3><div class="d-meta"><span class="d-pill" style="background:' + col + '">' + icon(stateIcon(p.state), "ti") + p.state.charAt(0).toUpperCase() + p.state.slice(1) + "</span>" + avatar(p.author) + "<b>" + p.author + '</b><i class="d-chip s">' + p.base + '</i><span class="d-arrow">←</span><i class="d-chip v">' + p.head + '</i><i class="g">+' + p.add + '</i><i class="r">−' + p.del + '</i><span class="d-ago">' + p.ago + " ago</span></div>";
    if (p.rerequest && !state.rerqHidden) {
      h += state.requested
        ? '<div class="d-rerq done">' + icon("check", "ti") + "<b>Requested</b><span>· Review re-requested from @" + p.rerequest.who + "</span></div>"
        : '<div class="d-rerq"><span>' + icon("commit", "ti") + "You pushed " + p.rerequest.n + " commits after @" + p.rerequest.who + " reviewed</span><button data-rerq>Re-request review</button></div>";
    }
    if (p.review) h += '<div class="d-fix"><span>' + icon("chat", "ti") + '1 unresolved comment</span><button data-fix>' + icon("spark", "ti") + "Fix with Claude Code</button></div>";
    h += "</div>";
    var tabs = [["conversation", "Conversation", "chat", p.review ? 1 : 0], ["commits", "Commits", "commit", p.commits.length], ["checks", "Checks", "check", p.checks.length], ["files", "Files changed", "file", p.files.length]];
    h += '<div class="d-tabs">' + tabs.map(function (t) {
      return '<button data-tab="' + t[0] + '" class="' + (state.tab === t[0] ? "on" : "") + '">' + icon(t[2], "ti") + t[1] + (t[3] ? "<em>" + t[3] + "</em>" : "") + "</button>";
    }).join("") + '<span class="d-ul"></span></div><div class="d-body">' + tabBody(p) + "</div></div>";
    return h;
  }

  function tabBody(p) {
    if (state.tab === "conversation") {
      var h = '<div class="d-conv"><div class="d-card"><div class="d-ch">' + avatar(p.author) + "<b>" + p.author + "</b> · description · " + p.ago + ' ago</div><div class="d-cb">' + p.body + "</div></div>";
      if (p.review) {
        h += '<div class="d-card"><div class="d-ch">' + avatar(p.review.who) + "<b>" + p.review.who + '</b> · <span class="r">requested changes</span></div><div class="d-cb"><div class="d-tpath">' + p.review.file + ":" + p.review.line + "</div>" + esc(p.review.text) + '<div class="d-acts"><button>Reply</button><button>Resolve</button><button class="d-hot" data-fix>' + icon("spark", "ti") + "Fix with Claude Code</button></div></div></div>";
      }
      return h + '<div class="d-composer">Comment (Markdown) — ⌘↩ to send<span>Comment</span></div></div>';
    }
    if (state.tab === "commits") {
      return '<div class="d-plain">' + p.commits.map(function (c, i) {
        return '<div class="d-line">' + icon("commit", "ti dim") + "<b>" + esc(c[0]) + "</b><small>" + avatar(c[1]) + c[1] + " · " + c[2] + ' ago</small><code>' + ["a3f9c21", "7b1e044", "c09d5af", "e4a7713"][i % 4] + "</code></div>";
      }).join("") + "</div>";
    }
    if (state.tab === "checks") {
      return '<div class="d-plain">' + p.checks.map(function (c) {
        var ic = c[0] === "ok" ? icon("check", "ti g") : c[0] === "fail" ? icon("x", "ti r") : icon("clock", "ti a");
        return '<div class="d-line">' + ic + "<b>" + c[1] + "</b><small>" + c[2] + "</small></div>";
      }).join("") + "</div>";
    }
    var f = p.files[Math.min(state.file, p.files.length - 1)];
    var h2 = '<div class="d-files"><div class="d-flist"><div class="d-ff">Filter files<span>' + p.files.length + "</span></div>";
    p.files.forEach(function (file, i) {
      var parts = file.path.split("/");
      h2 += '<button class="d-file' + (file === f ? " on" : "") + '" data-file="' + i + '"><i class="' + (file.st === "A" ? "g" : "a") + '">' + file.st + "</i><span><b>" + parts.pop() + "</b><small>" + parts.join("/") + "</small></span></button>";
    });
    h2 += '</div><div class="d-diff"><div class="d-fh">' + f.path + ' <i class="g">+' + f.add + '</i><i class="r">−' + f.del + '</i><span class="d-viewed"><i></i>Viewed</span></div>';
    f.lines.forEach(function (l) {
      h2 += '<div class="d-ln ' + l[0] + '"><span>' + l[1] + "</span><span>" + l[2] + "</span><span>" + (l[0] === "a" ? "+" : l[0] === "d" ? "−" : "") + "</span><span>" + esc(l[3]) + "</span></div>";
    });
    return h2 + "</div></div>";
  }

  function repoView() {
    var r = REPOS[state.repo];
    return '<div class="d-repo"><div class="d-card d-rhead"><span class="d-tile md" style="--c:#2dd4bf">' + icon("book", "ti") + "</span><div><b>" + r[0] + "</b><small>" + r[3] + '</small><div class="d-meta"><i class="d-chip a">★ ' + r[4] + '</i><i class="d-chip s">main</i></div></div></div><div class="d-tree">' +
      ["src", "tests", ".github"].map(function (d) { return '<div class="d-tr">' + icon("folder", "ti s") + d + "</div>"; }).join("") +
      ["README.md", "package.json", "tsconfig.json"].map(function (d) { return '<div class="d-tr">' + icon("file", "ti dim") + d + "</div>"; }).join("") +
      '</div><div class="d-card d-readme"><h4>' + r[0].split("/")[1] + "</h4><p>" + r[3] + ". Run <code>make dev</code> to start it locally.</p></div></div>";
  }

  function boardView() {
    var cols = [["Todo", "#94a3b8", ["Saved searches", "Offline mode"]], ["In progress", "#5bb8ec", ["Search debounce #482", "Dark settings #479"]], ["Done", "#9d8cf0", ["Empty list crash #77"]]];
    return '<div class="d-board">' + cols.map(function (c) {
      return '<div class="d-col"><div class="d-colh" style="border-top-color:' + c[1] + '">' + c[0] + "<em>" + c[2].length + "</em></div>" + c[2].map(function (t) { return '<div class="d-item">' + t + "</div>"; }).join("") + "</div>";
    }).join("") + "</div>";
  }

  var SCRIPT = [
    { k: "user", text: "Address the review on #482: cancel stale search requests when the query changes." },
    { k: "text", text: "I'll pass an <code>AbortSignal</code> to <code>fetchResults</code> and abort it when the effect re-runs." },
    { k: "tool", name: "Read", detail: "src/search/useSearch.ts" },
    { k: "edit" },
    { k: "perm" },
    { k: "bash" },
    { k: "text", text: "Done. Each query now gets its own <code>AbortController</code>, so a slow response can't overwrite newer results. All 12 tests pass." }
  ];

  function chatView() {
    var c = state.chat || { step: 0, items: [] };
    var h = '<div class="d-chat"><div class="d-ctab"><span class="d-ct">' + icon("chat", "ti") + 'Cancel stale searches <small>web-app</small></span><span class="d-cplus">+</span><span class="d-cmodel">opus</span></div><div class="d-msgs" id="d-msgs">';
    if (!c.items.length) h += '<div class="d-welcome"><span class="d-tile lg" style="--c:var(--coral)">' + icon("spark", "ti") + '</span><b>Claude Code</b><p>Open a PR and press <em>Fix with Claude Code</em>, or start here.</p><button class="d-start" data-fix>' + icon("spark", "ti") + "Fix review on #482</button></div>";
    c.items.forEach(function (it, i) { h += i === c.fresh ? it.replace('<div class="', '<div class="new ') : it; });
    if (c.busy) h += '<div class="d-working"><span class="d-spin"></span>Working…</div>';
    h += '</div><form class="d-ccomp" data-compose><input placeholder="Ask Claude or describe a task…" aria-label="Message"><div class="d-crow"><span>Accept edits ▾</span><span>Opus ▾</span><span>effort ▾</span><span class="d-ring"></span><button type="submit" aria-label="Send">' + icon("send", "ti") + "</button></div></form></div>";
    return h;
  }

  function chatItem(step) {
    if (step.k === "user") return '<div class="d-um">' + step.text + "</div>";
    if (step.k === "text") return '<div class="d-ans">' + step.text + "</div>";
    if (step.k === "tool") return '<div class="d-tool"><span class="d-ok">' + icon("check", "ti") + "</span><b>" + step.name + "</b><small>" + step.detail + "</small></div>";
    if (step.k === "edit") return '<div class="d-toolx"><div class="d-tool"><span class="d-ok">' + icon("check", "ti") + '</span><b>Edit</b><small>useSearch.ts</small><span class="d-st"><i class="g">+3</i> <i class="r">−1</i></span></div>' +
      '<div class="d-ln a"><span></span><span>24</span><span>+</span><span>    const ctrl = new AbortController()</span></div>' +
      '<div class="d-ln d"><span>25</span><span></span><span>−</span><span>    fetchResults(debounced)</span></div>' +
      '<div class="d-ln a"><span></span><span>25</span><span>+</span><span>    fetchResults(debounced, ctrl.signal)</span></div>' +
      '<div class="d-ln a"><span></span><span>29</span><span>+</span><span>    return () =&gt; ctrl.abort()</span></div></div>';
    if (step.k === "perm") return '<div class="d-perm" id="d-perm"><small>Claude wants to run a command</small><code>npm test -- search</code><div class="d-pacts"><button data-allow>Allow</button><button data-deny>Deny</button><kbd>⌘↩</kbd></div></div>';
    if (step.k === "bash") return '<div class="d-toolx"><div class="d-tool"><span class="d-ok">' + icon("check", "ti") + '</span><b>Bash</b><small>npm test -- search</small></div><pre> ✓ src/search/SearchBar.test.tsx (8)\n ✓ src/hooks/useDebounce.test.ts (4)\n\n Tests  12 passed</pre></div>';
    if (step.k === "denied") return '<div class="d-ans dim">Skipped the tests. Tell me what to do instead.</div>';
    return "";
  }

  var timer = null;
  function runChat() {
    var c = state.chat;
    if (!c || c.waiting) return;
    if (c.step >= SCRIPT.length) { c.busy = false; render(); return; }
    c.busy = true;
    render();
    var step = SCRIPT[c.step];
    timer = setTimeout(function () {
      c.items.push(chatItem(step));
      c.fresh = c.items.length - 1;
      c.step++;
      if (step.k === "perm") { c.waiting = true; c.busy = false; render(); return; }
      render();
      runChat();
    }, step.k === "user" ? 250 : 900);
  }

  function startFix() {
    clearTimeout(timer);
    state.section = "claude";
    state.chat = { step: 0, items: [], busy: false, waiting: false };
    state.detailOpen = true;
    render();
    runChat();
  }

  var lastDetailKey = null, lastTabKey = null, lastListKey = null;
  var reduced = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  function agoText() {
    var s = Math.max(0, Math.round((Date.now() - state.updated) / 1000));
    return s < 10 ? "just now" : s < 60 ? Math.floor(s / 5) * 5 + "s ago" : Math.floor(s / 60) + "m ago";
  }

  function render() {
    root.className = "demo" + (state.detailOpen ? " show-detail" : "");
    root.innerHTML = sidebar() + listColumn() + detail();
    var listKey = state.section + "|" + state.filter;
    if (listKey !== lastListKey) {
      Array.prototype.slice.call(root.querySelectorAll(".d-list .d-row"), 0, 12).forEach(function (r, i) {
        r.style.animationDelay = (reduced ? 0 : i * 70) + "ms";
        r.classList.add("d-in");
      });
      lastListKey = listKey;
    }
    if (state.fresh && !state.freshDone && root.querySelector(".d-new")) state.freshDone = true;
    var detailKey = [state.section, state.detail, state.pr, state.repo].join("|");
    var tabKey = detailKey + "|" + state.tab + "|" + state.file;
    var det = root.querySelector(".d-det"), body = root.querySelector(".d-body");
    if (det && detailKey !== lastDetailKey && lastDetailKey !== null) det.classList.add("enter");
    else if (body && tabKey !== lastTabKey && lastTabKey !== null) body.classList.add("enter");
    lastDetailKey = detailKey;
    lastTabKey = tabKey;
    var msgs = document.getElementById("d-msgs");
    if (msgs) msgs.scrollTop = msgs.scrollHeight;
    var ul = root.querySelector(".d-ul"), on = root.querySelector(".d-tabs .on");
    if (ul && on) { ul.style.width = on.offsetWidth + "px"; ul.style.transform = "translateX(" + on.offsetLeft + "px)"; }
  }

  root.addEventListener("click", function (e) {
    var t = e.target.closest("button");
    if (!t || !root.contains(t)) return;
    if (t.dataset.sec) {
      state.section = t.dataset.sec;
      state.detailOpen = false;
      var s = sectionById(state.section);
      if (s.list) { state.filter = "open"; state.detail = s.list.indexOf(state.pr) >= 0 && state.detail === "pr" ? "pr" : null; }
      if (state.section === "repos") state.detail = null;
      if (state.section === "claude" && !state.chat) state.chat = { step: 0, items: [] };
      render();
    } else if (t.dataset.pr) {
      state.pr = +t.dataset.pr; state.detail = "pr"; state.tab = "conversation"; state.file = 0; state.detailOpen = true;
      if (state.section !== "notifications" && !sectionById(state.section).list) state.section = "review";
      render();
    } else if (t.dataset.repo) {
      state.repo = +t.dataset.repo; state.detail = "repo"; state.detailOpen = true; render();
    } else if (t.dataset.filter) {
      state.filter = t.dataset.filter; render();
    } else if (t.dataset.tab) {
      state.tab = t.dataset.tab; render();
    } else if (t.dataset.file) {
      state.file = +t.dataset.file; render();
    } else if (t.hasAttribute("data-rerq")) {
      state.requested = true; render();
      setTimeout(function () { state.rerqHidden = true; render(); }, 2500);
    } else if (t.hasAttribute("data-fix")) {
      startFix();
    } else if (t.hasAttribute("data-back")) {
      state.detailOpen = false; render();
    } else if (t.hasAttribute("data-allow") || t.hasAttribute("data-deny")) {
      var c = state.chat;
      c.waiting = false;
      c.items.pop();
      if (t.hasAttribute("data-deny")) { c.items.push(chatItem({ k: "denied" })); c.fresh = c.items.length - 1; c.step = SCRIPT.length; c.busy = false; render(); return; }
      runChat();
    }
  });

  root.addEventListener("submit", function (e) {
    e.preventDefault();
    var input = e.target.querySelector("input");
    var text = (input.value || "").trim();
    if (!text) return;
    if (!state.chat) state.chat = { step: SCRIPT.length, items: [] };
    state.chat.items.push('<div class="d-um">' + esc(text) + "</div>");
    state.chat.items.push('<div class="d-ans">This is a live demo with a scripted session. Download PullYard to run Claude Code on your own repositories.</div>');
    state.chat.fresh = state.chat.items.length - 1;
    render();
  });

  setInterval(function () {
    var ago = root.querySelector("[data-ago]");
    if (ago) ago.textContent = agoText();
  }, 5000);

  setTimeout(function () {
    var review = sectionById("review");
    if (review.list.indexOf(91) >= 0) return;
    review.list.unshift(91);
    review.count += 1;
    state.fresh = 91;
    state.freshDone = false;
    state.updated = Date.now();
    if (state.section === "review") render();
  }, 6000);

  state.detail = "pr";
  render();
})();
