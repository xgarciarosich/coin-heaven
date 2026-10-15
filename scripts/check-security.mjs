#!/usr/bin/env node
// Static security guard for Coin Heaven. Needs nothing but Node.
//
//   node scripts/check-security.mjs                 check this repository
//   node scripts/check-security.mjs --self-test     prove every rule still fires
//                                                   (--verbose lists every case)
//   node scripts/check-security.mjs --strict-pins   unpinned actions fail, not warn
//   node scripts/check-security.mjs --root <dir>    check another folder
//
// `pnpm check:security` runs the self-test first and then the check, so a
// regex that was quietly edited into uselessness fails here instead of passing
// every build afterwards.
//
// The threat model says the webview has no network, no disk, and no way to
// run code it was not built with. These checks keep that true by failing the
// build when a change would quietly undo it. They are text checks: they find
// the obvious regressions, not a determined attacker. Review still matters.
//
// WHAT FAILS (exit code 1)
//   Frontend (src/ and public/)
//     no-html-injection     dangerouslySetInnerHTML, innerHTML/outerHTML
//                           assignment, insertAdjacentHTML
//     no-dynamic-code       eval, new Function, document.write, setTimeout("text")
//     no-network            fetch, XMLHttpRequest, WebSocket, EventSource,
//                           sendBeacon, imports from a URL
//     no-http-url           http:// or ws:// to anything but this machine
//     no-external-asset     index.html or a stylesheet loading from a remote host
//                           (inline <script> bodies in index.html get the
//                           code and network rules too)
//   Tauri configuration (src-tauri/tauri*.conf.json)
//     csp-missing, csp-unsafe-eval, csp-wildcard, csp-remote-source,
//     csp-unsafe-inline-script, csp-weakened, csp-unreviewed-source
//     tauri-dangerous       switches Tauri itself names "dangerous", a global
//                           API object, or a window that loads a remote page
//   Capabilities (src-tauri/capabilities/)
//     capability-denied-permission   fs, shell, http, process, os, opener ...
//     capability-core-default        core:default is not the baseline any more
//     capability-unreviewed          any permission not listed below
//     capability-remote, capability-wildcard-window
//   Supply chain
//     lifecycle-script      preinstall / install / postinstall / prepare
//     non-registry-dependency   a dependency fetched from git or a URL
//     workflow-pull-request-target
//     workflow-expression-injection   ${{ github.event.* }} inside run:
//     workflow-unpinned-action        (a warning until the actions are pinned)
//   Leaks, in every tracked file
//     private-path          a home-folder path of a real person
//     private-email         an address that is not a noreply or example one
//     build-output-not-ignored   node_modules, dist or target that git would
//                           publish (these folders are not read, so this is
//                           the only thing that would notice them)
//
// WHAT ONLY WARNS (exit code 0, still printed)
//   dynamic-href            a link whose address comes from data
//   history-email           a commit author address that is not a noreply one
//   workflow-unpinned-action
//
// A frontend finding can be accepted on the spot with a comment on the same
// line or the line above:
//
//     // security-allow: no-network -- why this one is safe
//
// The reason is required. Accepted lines are listed in every run, so a review
// can see them. Configuration rules have no marker: change the allow-lists
// below instead, which is a diff someone has to approve.
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// ---------------------------------------------------------------------------
// The allow-lists. Everything a reviewer might want to argue with is here.
// ---------------------------------------------------------------------------

/** Webview permissions that have been reviewed. A new one fails until it is
 *  added here, in the same change that grants it. */
const ALLOWED_PERMISSIONS = new Set([
  "core:default",
  "core:window:allow-start-dragging",
  "dialog:allow-open",
  "dialog:allow-save",
]);

/** Plugins that reach the disk, the network or other programs. The webview
 *  gets none of them: privileged work is a #[tauri::command] in Rust. */
const DENIED_PERMISSION_PREFIXES = [
  "fs", "shell", "http", "process", "os", "opener",
  "websocket", "sql", "stronghold", "upload", "updater",
];

/** Where the webview may be pointed at while developing. */
const LOCAL_HOST = /^(?:localhost|127\.0\.0\.1|\[::1\]|[\w-]+\.localhost)$/i;

/** Addresses nobody is exposed by publishing. Exact addresses a project
 *  mailbox needs to appear in the repository go in ALLOWED_EMAILS. */
const ALLOWED_EMAILS = new Set([]);
const RESERVED_DOMAINS = /(?:^|\.)(?:example\.(?:com|net|org)|example|test|invalid|localhost)$/i;
const NOREPLY_DOMAINS = /(?:^|\.)noreply\.github\.com$/i;
const NOREPLY_LOCALS = /^(?:no-?reply|do-?not-?reply)$/i;
const GIT_HOSTS = new Set(["github.com", "gitlab.com", "bitbucket.org", "codeberg.org"]);
// "icon@2x.png" is a file name, not a mailbox.
const RETINA_NAME = /^\d+x\.(?:png|jpe?g|webp|gif|svg)$/i;
/** Machine-written files: their "@" signs are package versions, not mailboxes. */
const GENERATED = /(?:^|\/)(?:pnpm-lock\.yaml|package-lock\.json|yarn\.lock|Cargo\.lock)$/;

/** Names that show a path is an example, not a person. */
const PLACEHOLDER_USER =
  /^(?:<[^>]+>|\{[^}]+\}|\$\{?\w+\}?|%\w+%|(?:you|me|name|user|username|your-?name|your-?user|example|someone|shared)(?![\w-]))/i;

// Built from pieces so this file does not trip its own check.
const MAC_HOME = ["", "Users", ""].join("/");
const WIN_HOME = "\\Users\\";

/** Floating action tags fail instead of warn once they are all pinned. */
const REQUIRE_SHA_PINS = false;

// ---------------------------------------------------------------------------
// Findings
// ---------------------------------------------------------------------------

class Findings {
  constructor() {
    /** @type {{level:"fail"|"warn", rule:string, file:string, line:number|null, message:string, snippet?:string}[]} */
    this.items = [];
    /** @type {{rule:string, file:string, line:number, reason:string}[]} */
    this.allowed = [];
  }
  add(level, rule, file, line, message, snippet) {
    this.items.push({ level, rule, file, line, message, snippet });
  }
  fail(rule, file, message, line = null, snippet) {
    this.add("fail", rule, file, line, message, snippet);
  }
  warn(rule, file, message, line = null, snippet) {
    this.add("warn", rule, file, line, message, snippet);
  }
}

const MARKER = /security-allow:\s*([a-z][a-z0-9-]*)\s*(?:--|—|–|:|-)?\s*(.*)$/i;

/** The accepted-with-a-reason marker on this line or the one above. */
function findMarker(lines, i, rule) {
  for (const j of [i, i - 1]) {
    if (j < 0) continue;
    const m = MARKER.exec(lines[j]);
    if (m && m[1].toLowerCase() === rule) {
      const reason = m[2].replace(/(?:\*\/\}?|--!?>)\s*$/, "").trim();
      return { reason: reason.length >= 10 ? reason : "" };
    }
  }
  return null;
}

function flagLine(f, level, rule, file, lines, i, message) {
  const marker = findMarker(lines, i, rule);
  if (marker?.reason) {
    f.allowed.push({ rule, file, line: i + 1, reason: marker.reason });
    return;
  }
  const note = marker ? " A security-allow marker is here, but it needs a reason after the rule name." : "";
  f.add(level, rule, file, i + 1, message + note, lines[i].trim().slice(0, 140));
}

const splitLines = (text) => text.split(/\r?\n/);

// ---------------------------------------------------------------------------
// Frontend: src/ and public/
// ---------------------------------------------------------------------------

const isFrontend = (file) => file.startsWith("src/") || file.startsWith("public/");
const isCode = (file) => /\.[cm]?[jt]sx?$/.test(file);
const isTestFile = (file) =>
  /\.(?:test|spec)\.[cm]?[jt]sx?$/.test(file) || /(?:^|\/)(?:__tests__|tests?)\//.test(file);

const CODE_RULES = [
  {
    rule: "no-html-injection",
    message:
      "Writes markup into the page. Text from a statement, a merchant or a model must reach the screen as text; render it as a React child instead.",
    patterns: [
      /dangerouslySetInnerHTML/,
      /\b(?:inner|outer)HTML\s*(?:\+|\|\|)?=(?!=)/,
      /\[\s*["'`](?:inner|outer)HTML["'`]\s*\]\s*(?:\+|\|\|)?=(?!=)/,
      /\binsertAdjacentHTML\s*\(/,
    ],
  },
  {
    rule: "no-dynamic-code",
    message:
      "Runs a string as code. Nothing in the UI needs it, and the content policy would block it in the app anyway.",
    patterns: [
      /\beval\s*\(/,
      /\(\s*0\s*,\s*(?:window\.|globalThis\.)?eval\s*\)/,
      /\[\s*["'`]eval["'`]\s*\]/,
      /\bnew\s+Function\b/,
      /\bFunction\(/,
      /\bdocument\s*\.\s*write(?:ln)?\s*\(/,
      /\bset(?:Timeout|Interval)\s*\(\s*["'`]/,
    ],
  },
  {
    rule: "no-network",
    message:
      "The interface has no network access, by design. Ask the Rust side through a #[tauri::command] so the request is visible, previewed and logged.",
    patterns: [
      /\bfetch\s*\(/,
      /\bXMLHttpRequest\b/,
      /\bWebSocket\b/,
      /\bEventSource\b/,
      /\bsendBeacon\b/,
      /(?:\bfrom\s+|\bimport\s*\(?\s*)["'`]\s*(?:https?:)?\/\//,
    ],
  },
];

// An address up to the first character that cannot be part of it; an IPv6
// host keeps its brackets.
const URL_TAIL = String.raw`((?:\[[0-9a-f:.]*\])?[^\s"'\x60<>)\]}\\,;]*)`;
const PLAIN_URL = new RegExp(String.raw`\b(?:http|ws):\/\/` + URL_TAIL, "gi");
const ANY_URL = new RegExp(String.raw`\b(?:https?|wss?):\/\/` + URL_TAIL, "gi");

/** The first address on the line that points away from this machine, or null.
 *  By default only unencrypted ones (http, ws); `any` includes https and wss. */
function firstRemoteUrl(line, any = false) {
  for (const m of line.matchAll(any ? ANY_URL : PLAIN_URL)) {
    const authority = m[1].split(/[/?#]/)[0];
    const hostPort = authority.slice(authority.lastIndexOf("@") + 1);
    const host = hostPort.startsWith("[")
      ? hostPort.slice(0, hostPort.indexOf("]") + 1)
      : hostPort.split(":")[0];
    if (!host.includes("${") && LOCAL_HOST.test(host)) continue;
    return m[0];
  }
  return null;
}

const REMOTE_URL = /^\s*(?:(?:https?|wss?|ftp):)?\/\//i;
const REMOTE_SCHEME = /^\s*(?:https?|wss?|ftp):/i;
const isRemote = (v) => REMOTE_URL.test(v) || REMOTE_SCHEME.test(v);

function checkHtmlAssets(f, file, text) {
  const tag = /<(script|link|iframe|img|object|embed|source|video|audio|form|base|input)\b[^>]*>/gis;
  const attr = /\b(src|href|action|data|poster|srcset)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/gis;
  for (const t of text.matchAll(tag)) {
    for (const a of t[0].matchAll(attr)) {
      const value = a[2] ?? a[3] ?? a[4] ?? "";
      if (!isRemote(value)) continue;
      const line = text.slice(0, t.index).split("\n").length;
      f.fail(
        "no-external-asset",
        file,
        `<${t[1].toLowerCase()} ${a[1].toLowerCase()}> loads from a remote host. Ship the file with the app instead; the window must not depend on a server.`,
        line,
        a[0].slice(0, 140),
      );
    }
  }
}

function checkCss(f, file, text) {
  const lines = splitLines(text);
  lines.forEach((line, i) => {
    if (/(?:@import\s+(?:url\()?\s*|url\(\s*)["']?\s*(?:(?:https?|ftp):)?\/\//i.test(line)) {
      f.fail(
        "no-external-asset",
        file,
        "A stylesheet pulls something from a remote host. Bundle the font or image with the app.",
        i + 1,
        line.trim().slice(0, 140),
      );
    }
  });
}

/** The page with everything except the bodies of inline <script> tags blanked
 *  out, newlines kept, so the code rules can run on it with true line numbers.
 *  Data blocks (type="application/json") are not code and are left blank. */
function inlineScripts(html) {
  const out = html.replace(/[^\n]/g, " ").split("");
  for (const m of html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script[^>]*>/gi)) {
    const attrs = m[1];
    if (/\bsrc\s*=/i.test(attrs)) continue;
    const type = /\btype\s*=\s*["']?([^\s"'>]+)/i.exec(attrs)?.[1];
    if (type && !/^(?:module|(?:text|application)\/(?:java|ecma)script)$/i.test(type)) continue;
    const start = m.index + "<script".length + attrs.length + 1;
    for (let k = 0; k < m[2].length; k++) out[start + k] = html[start + k];
  }
  return out.join("");
}

/** The code rules, one line at a time. */
function scanCode(f, file, text) {
  const lines = splitLines(text);
  lines.forEach((line, i) => {
    for (const { rule, message, patterns } of CODE_RULES) {
      if (patterns.some((re) => re.test(line))) flagLine(f, "fail", rule, file, lines, i, message);
    }
    if (!isTestFile(file)) {
      const url = firstRemoteUrl(line);
      if (url) {
        flagLine(
          f, "fail", "no-http-url", file, lines, i,
          `${url} is plain http (or a websocket) to another machine. Only this computer may be addressed without TLS, and the UI has no network at all.`,
        );
      }
    }
    // A link built from data: the scheme has to be checked before it is shown.
    if (/\bhref\s*=\s*\{\s*(?!["']|`(?!\$\{))/.test(line)) {
      flagLine(
        f, "warn", "dynamic-href", file, lines, i,
        "A link whose address comes from data (a model or a web page can supply it). Show it only when it starts with https:// or http://, or accept it with a security-allow marker that says where that is checked.",
      );
    }
  });
}

function checkFrontend(f, files) {
  for (const [file, text] of files) {
    if (file === "index.html" || (isFrontend(file) && file.endsWith(".html"))) {
      checkHtmlAssets(f, file, text);
      // The policy blocks inline script in the app, but a page that carries one
      // is still wrong and would work in the dev server.
      scanCode(f, file, inlineScripts(text));
      continue;
    }
    if (!isFrontend(file)) continue;
    if (file.endsWith(".css")) {
      checkCss(f, file, text);
      continue;
    }
    if (isCode(file)) scanCode(f, file, text);
  }
}

// ---------------------------------------------------------------------------
// Tauri configuration and capabilities
// ---------------------------------------------------------------------------

const TAURI_CONFIG = /^src-tauri\/tauri(?:\.[a-z0-9-]+)?\.conf\.json5?$/i;

/** Every directive that decides where the page may fetch something from. */
const FETCH_DIRECTIVES = new Set([
  "default-src", "script-src", "script-src-elem", "script-src-attr", "style-src",
  "style-src-elem", "style-src-attr", "img-src", "font-src", "media-src",
  "connect-src", "frame-src", "child-src", "worker-src", "manifest-src",
  "object-src", "prefetch-src",
]);

function parseCsp(value) {
  const out = [];
  if (typeof value === "string") {
    for (const part of value.split(";")) {
      const tokens = part.trim().split(/\s+/).filter(Boolean);
      if (tokens.length) out.push({ name: tokens[0].toLowerCase(), sources: tokens.slice(1) });
    }
  } else if (value && typeof value === "object" && !Array.isArray(value)) {
    for (const [name, v] of Object.entries(value)) {
      const sources = Array.isArray(v) ? v.map(String) : String(v).trim().split(/\s+/).filter(Boolean);
      out.push({ name: name.toLowerCase(), sources });
    }
  }
  return out;
}

const DEV_LOCAL_SOURCE = /^(?:https?|wss?):\/\/(?:localhost|127\.0\.0\.1|\[::1\])(?::\d{1,5})?\/?$/i;

/** What is wrong with one source in one directive, or null if it is fine.
 *  `dev` is the devCsp, which may talk to the Vite server on this machine. */
function cspSourceProblem(directive, source, dev) {
  const s = source.toLowerCase();
  if (s === "'self'" || s === "'none'") return null;
  if (s === "'unsafe-eval'") {
    return ["csp-unsafe-eval", "'unsafe-eval' lets any string run as code, which turns an injected string into an attack."];
  }
  if (s.includes("*")) {
    return ["csp-wildcard", `${source} is a wildcard; it allows hosts nobody reviewed.`];
  }
  if (/^(?:https?|wss?|ftp):$/.test(s)) {
    return ["csp-remote-source", `${source} allows any website. Data could leave through it.`];
  }
  if (directive === "connect-src") {
    if (s === "ipc:" || s === "ipc://localhost" || s === "http://ipc.localhost") return null;
    if (dev && DEV_LOCAL_SOURCE.test(s)) return null;
  }
  if (/^(?:https?|wss?|ftp):\/\//.test(s) || /^[\w-]+(?:\.[\w-]+)+(?::\d+)?(?:\/.*)?$/.test(s) || /^localhost(?::\d+)?$/.test(s)) {
    const where = dev && directive === "connect-src" ? "" : " (a development server belongs in devCsp connect-src only)";
    return ["csp-remote-source", `${source} is a host other than the app itself${where}.`];
  }
  if (s === "'unsafe-inline'") {
    if (directive.startsWith("style-src")) return null;
    if (dev && (directive === "script-src" || directive === "script-src-elem")) return null; // the Vite React preamble
    return ["csp-unsafe-inline-script", `'unsafe-inline' in ${directive} lets an injected tag run as script.`];
  }
  if (s === "data:" && ["img-src", "font-src", "media-src"].includes(directive)) return null;
  if (/^'(?:nonce|sha256|sha384|sha512)-/.test(s) && /^(?:script|style)-src/.test(directive)) return null;
  return ["csp-unreviewed-source", `${source} in ${directive} has not been reviewed. Add it to cspSourceProblem() with the reason, in the same change.`];
}

function checkCsp(f, file, label, value, { dev, required }) {
  if (value === undefined || value === null || value === "") {
    if (required) {
      f.fail("csp-missing", file, `${label} is not set. Without it the webview has no content policy at all.`);
    }
    return;
  }
  const directives = parseCsp(value);
  if (directives.length === 0) {
    f.fail("csp-missing", file, `${label} could not be read as a content policy.`);
    return;
  }
  for (const { name, sources } of directives) {
    if (FETCH_DIRECTIVES.has(name)) {
      for (const source of sources) {
        const problem = cspSourceProblem(name, source, dev);
        if (problem) f.fail(problem[0], file, `${label}: ${problem[1]}`);
      }
    }
    if (["report-uri", "report-to", "navigate-to"].includes(name) && sources.length > 0) {
      f.fail("csp-remote-source", file, `${label}: ${name} sends violation reports or navigates somewhere else; remove it.`);
    }
  }
  const first = (name) => directives.find((d) => d.name === name);
  const onlySelfOrNone = (d) => d.sources.length > 0 && d.sources.every((s) => ["'self'", "'none'"].includes(s.toLowerCase()));

  if (!first("default-src")) {
    f.fail("csp-weakened", file, `${label}: default-src is missing, so every directive not listed is unrestricted.`);
  } else if (!onlySelfOrNone(first("default-src"))) {
    f.fail("csp-weakened", file, `${label}: default-src must be 'self' (or 'none').`);
  }
  for (const name of ["object-src", "frame-ancestors"]) {
    const d = first(name);
    if (!d || d.sources.length !== 1 || d.sources[0].toLowerCase() !== "'none'") {
      f.fail("csp-weakened", file, `${label}: ${name} must be exactly 'none'${d ? `, found ${d.sources.join(" ") || "nothing"}` : " and it is missing"}.`);
    }
  }
  for (const name of ["base-uri", "form-action"]) {
    const d = first(name);
    if (!d || !onlySelfOrNone(d)) {
      f.fail("csp-weakened", file, `${label}: ${name} must be 'self' or 'none'${d ? "" : " and it is missing"}.`);
    }
  }
}

/** "*", "**" or a scheme and nothing else: matches every address. */
const matchesEveryAddress = (s) => typeof s === "string" && /^(?:[a-z*]+:\/\/)?\*{1,2}\/?$/i.test(s.trim());
/** "**", "/**" or a whole well-known folder: matches every file a person has. */
const matchesEveryFile = (s) =>
  typeof s === "string" &&
  /^(?:\*{1,2}|\/\*{1,2}|\$(?:HOME|DOCUMENT|DESKTOP|DOWNLOAD|APPDATA|LOCALDATA)\/\*{1,2})$/.test(s.trim());

function checkTauriConfig(f, file, conf, isBase) {
  const security = conf?.app?.security ?? {};
  checkCsp(f, file, "csp", security.csp, { dev: false, required: isBase });
  checkCsp(f, file, "devCsp", security.devCsp, { dev: true, required: false });

  if (conf?.app?.withGlobalTauri === true) {
    f.fail("tauri-dangerous", file, "app.withGlobalTauri is true: every script in the page gets the whole Tauri API on window. The app imports what it needs.");
  }
  if (security.dangerousDisableAssetCspModification) {
    f.fail("tauri-dangerous", file, "dangerousDisableAssetCspModification stops Tauri from tightening the policy on bundled files.");
  }
  if (Array.isArray(security.dangerousRemoteDomainIpcAccess) && security.dangerousRemoteDomainIpcAccess.length > 0) {
    f.fail("tauri-dangerous", file, "dangerousRemoteDomainIpcAccess gives a website the same commands as the app.");
  }
  const asset = security.assetProtocol;
  if (asset?.enable === true) {
    const scope = Array.isArray(asset.scope) ? asset.scope : [...(asset.scope?.allow ?? [])];
    if (scope.length === 0 || scope.some(matchesEveryFile)) {
      f.fail("tauri-dangerous", file, "assetProtocol is on with an unrestricted scope: the webview could read any file the user can.");
    }
  }
  const remoteUrl = (v) => typeof v === "string" && firstRemoteUrl(v, true) !== null;
  const windows = Array.isArray(conf?.app?.windows) ? conf.app.windows : [];
  for (const w of windows) {
    if (remoteUrl(w?.url)) f.fail("tauri-dangerous", file, `A window loads ${w.url}. The app's own pages are the only thing the window may show.`);
  }
  if (remoteUrl(conf?.build?.frontendDist)) {
    f.fail("tauri-dangerous", file, "build.frontendDist is a remote address, so the app would run a website.");
  }
  if (remoteUrl(conf?.build?.devUrl)) {
    f.fail("tauri-dangerous", file, "build.devUrl points away from this machine.");
  }
  for (const entry of Array.isArray(security.capabilities) ? security.capabilities : []) {
    if (entry && typeof entry === "object") checkCapability(f, file, entry, `inline capability "${entry.identifier ?? "?"}"`);
  }
}

/** An opener permission with no narrow scope may open any address. */
function openerIsUnrestricted(p) {
  if (typeof p === "string") return true;
  const allow = Array.isArray(p?.allow) ? p.allow : [];
  if (allow.length === 0) return true;
  return allow.some((e) => matchesEveryAddress(e?.url) || matchesEveryFile(e?.path));
}

function checkCapability(f, file, cap, label) {
  const permissions = Array.isArray(cap?.permissions) ? cap.permissions : [];
  const ids = permissions.map((p) => (typeof p === "string" ? p : p?.identifier)).filter((s) => typeof s === "string");

  if (!ids.includes("core:default")) {
    f.fail("capability-core-default", file, `${label} does not grant core:default. It is the reviewed baseline; swapping it for other core permissions is how a window gains the ability to create windows or load pages.`);
  }
  permissions.forEach((p) => {
    const id = typeof p === "string" ? p : p?.identifier;
    if (typeof id !== "string") return;
    const prefix = id.split(":")[0].toLowerCase();
    if (DENIED_PERMISSION_PREFIXES.includes(prefix)) {
      if (prefix !== "opener" || openerIsUnrestricted(p)) {
        f.fail("capability-denied-permission", file, `${label} grants ${id}. The webview gets no file, shell, network, process or opener access; a privileged action is a reviewed #[tauri::command].`);
        return;
      }
    }
    if (!ALLOWED_PERMISSIONS.has(id)) {
      f.fail("capability-unreviewed", file, `${label} grants ${id}, which is not in ALLOWED_PERMISSIONS in scripts/check-security.mjs. Add it there, with the reason, in the same change.`);
    }
  });
  if (cap?.remote && (Array.isArray(cap.remote.urls) ? cap.remote.urls.length > 0 : true)) {
    f.fail("capability-remote", file, `${label} grants commands to remote pages. Only the app's own window may call into Rust.`);
  }
  for (const key of ["windows", "webviews"]) {
    if (Array.isArray(cap?.[key]) && cap[key].some((w) => String(w).includes("*"))) {
      f.fail("capability-wildcard-window", file, `${label} applies to ${key} "*". Name the window ("main") so a window added later starts with nothing.`);
    }
  }
}

function checkTauri(f, files, parse) {
  const configs = [...files.keys()].filter((p) => TAURI_CONFIG.test(p));
  if (!files.has("src-tauri/tauri.conf.json")) {
    f.fail("missing-file", "src-tauri/tauri.conf.json", "Not found, so the content policy cannot be checked.");
  }
  if ([...files.keys()].some((p) => /^src-tauri\/Tauri\.toml$/i.test(p))) {
    f.fail("config-unreadable", "src-tauri/Tauri.toml", "A TOML Tauri configuration overrides the JSON one and this check cannot read it. Keep the configuration in tauri.conf.json.");
  }
  for (const file of configs) {
    const conf = parse(file);
    if (conf) checkTauriConfig(f, file, conf, file === "src-tauri/tauri.conf.json");
  }

  const caps = [...files.keys()].filter((p) => p.startsWith("src-tauri/capabilities/"));
  if (caps.length === 0) {
    f.fail("missing-file", "src-tauri/capabilities/", "No capability file found. Without one the window's permissions cannot be checked.");
  }
  for (const file of caps) {
    if (!/\.json5?$/.test(file)) {
      f.fail("config-unreadable", file, "Capabilities are checked in JSON only. Convert this file so it can be reviewed by this check.");
      continue;
    }
    const value = parse(file);
    if (value === undefined) continue;
    const list = Array.isArray(value) ? value : Array.isArray(value?.capabilities) ? value.capabilities : [value];
    list.forEach((cap, i) => checkCapability(f, file, cap, `capability "${cap?.identifier ?? i}"`));
  }
}

// ---------------------------------------------------------------------------
// Dependencies and workflows
// ---------------------------------------------------------------------------

const LIFECYCLE = ["preinstall", "install", "postinstall", "prepublish", "preprepare", "prepare", "postprepare"];
const NON_REGISTRY = /^(?:git\+|git:|github:|gitlab:|bitbucket:|https?:|[\w.-]+\/[\w.-]+(?:#\S*)?$)/i;

function checkPackage(f, pkg) {
  const file = "package.json";
  for (const name of LIFECYCLE) {
    if (typeof pkg?.scripts?.[name] === "string") {
      f.fail("lifecycle-script", file, `scripts.${name} runs on every "pnpm install", before anyone has read what was installed. Use an explicit script that is run on purpose.`);
    }
  }
  const tables = [
    pkg?.dependencies, pkg?.devDependencies, pkg?.optionalDependencies, pkg?.peerDependencies,
    pkg?.overrides, pkg?.resolutions, pkg?.pnpm?.overrides,
  ];
  for (const table of tables) {
    if (!table || typeof table !== "object") continue;
    for (const [name, spec] of Object.entries(table)) {
      if (typeof spec === "string" && NON_REGISTRY.test(spec)) {
        f.fail("non-registry-dependency", file, `${name} is fetched from ${spec}, not the registry, so the lockfile cannot vouch for what it contains.`);
      }
    }
  }
}

const FULL_SHA = /^[0-9a-f]{40}$/i;

/** The `run:` and `script:` values of a workflow, with their line numbers. A
 *  block runs from the key to the next line indented no further than it. */
function* scriptBlocks(lines) {
  const key = /^(\s*)(-\s+)?(run|script)\s*:\s*(.*)$/;
  for (let i = 0; i < lines.length; i++) {
    const m = key.exec(lines[i]);
    if (!m) continue;
    const col = m[1].length + (m[2] ? m[2].length : 0);
    const block = [{ no: i, text: m[4] }];
    for (let j = i + 1; j < lines.length; j++) {
      if (lines[j].trim() !== "" && /^\s*/.exec(lines[j])[0].length <= col) break;
      block.push({ no: j, text: lines[j] });
    }
    yield block;
  }
}

function checkWorkflow(f, file, text, strictPins) {
  const lines = splitLines(text);
  const isComment = (l) => l.trim().startsWith("#");

  lines.forEach((line, i) => {
    if (!isComment(line) && /\bpull_request_target\b/.test(line)) {
      f.fail("workflow-pull-request-target", file, "pull_request_target runs with the repository's secrets on code from a fork. Use pull_request, which gets no secrets.", i + 1, line.trim());
    }
  });

  const expression = /\$\{\{[^}]*\bgithub\.(?:event\.|head_ref\b)[^}]*\}\}/;
  for (const block of scriptBlocks(lines)) {
    for (const { no, text: t } of block) {
      if (expression.test(t)) {
        f.fail("workflow-expression-injection", file, "A github.event or head_ref value is pasted into a script. Whoever opens the issue or branch controls that text. Pass it through env: and read it as a variable.", no + 1, t.trim().slice(0, 140));
      }
    }
  }

  const grouped = new Map();
  lines.forEach((line, i) => {
    if (isComment(line)) return;
    const m = /^\s*(?:-\s+)?uses\s*:\s*["']?([^"'\s#]+)/.exec(line);
    if (!m) return;
    const target = m[1];
    if (target.startsWith("./")) return;
    const at = target.lastIndexOf("@");
    const ref = at === -1 ? "" : target.slice(at + 1);
    const pinned = target.startsWith("docker://") ? /@sha256:[0-9a-f]{64}$/i.test(target) : FULL_SHA.test(ref);
    if (pinned) return;
    if (!grouped.has(target)) grouped.set(target, []);
    grouped.get(target).push(i + 1);
  });
  for (const [target, at] of grouped) {
    const message = `${target} follows a tag that its owner can move. Pin it to the 40-character commit (keep the tag in a trailing comment).`;
    const level = strictPins || REQUIRE_SHA_PINS ? "fail" : "warn";
    f.add(level, "workflow-unpinned-action", file, at[0], at.length > 1 ? `${message} Also at lines ${at.slice(1).join(", ")}.` : message);
  }
}

// ---------------------------------------------------------------------------
// Leaks in tracked files
// ---------------------------------------------------------------------------

const maskLocal = (s) => `${s.slice(0, 1)}***`;

function emailIsFine(address) {
  const lower = address.toLowerCase();
  if (ALLOWED_EMAILS.has(lower)) return true;
  const at = lower.lastIndexOf("@");
  const local = lower.slice(0, at);
  const domain = lower.slice(at + 1);
  return (
    NOREPLY_LOCALS.test(local) ||
    NOREPLY_DOMAINS.test(domain) ||
    RESERVED_DOMAINS.test(domain) ||
    RETINA_NAME.test(domain) ||
    (local === "git" && GIT_HOSTS.has(domain))
  );
}

const EMAIL_RE = /[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/g;

function checkLeaks(f, files) {
  for (const [file, text] of files) {
    const lines = splitLines(text);
    lines.forEach((line, i) => {
      for (const [home, sep] of [[MAC_HOME, "/"], [WIN_HOME, "\\"]]) {
        let from = 0;
        for (;;) {
          const at = line.indexOf(home, from);
          if (at === -1) break;
          from = at + home.length;
          const name = line.slice(from).split(sep)[0].split(/[\s"'`]/)[0];
          if (name === "" || PLACEHOLDER_USER.test(name)) continue;
          f.fail("private-path", file, `A home-folder path that names a real account (${home}${maskLocal(name)}...). Publishing it publishes a username; use ~ or a placeholder.`, i + 1);
        }
      }
      if (GENERATED.test(file)) return;
      for (const m of line.matchAll(EMAIL_RE)) {
        if (emailIsFine(m[0])) continue;
        const at = m[0].lastIndexOf("@");
        f.fail("private-email", file, `An email address (${maskLocal(m[0].slice(0, at))}${m[0].slice(at)}) that is not a noreply or example one. Use a noreply address, or add this one to ALLOWED_EMAILS if it is a project mailbox meant to be public.`, i + 1);
      }
    });
  }
}

/** Build folders are not read, so a folder git would publish is a finding in
 *  itself: a Rust build leaves the builder's home folder in every .d file. */
function checkBuildOutput(f, paths) {
  const perDir = new Map();
  for (const rel of paths ?? []) {
    const dir = buildDirOf(rel);
    if (dir) perDir.set(dir, (perDir.get(dir) ?? 0) + 1);
  }
  for (const [dir, n] of perDir) {
    f.fail(
      "build-output-not-ignored",
      `${dir}/`,
      `${n} file${n === 1 ? "" : "s"} here are not covered by .gitignore, so "git add ." would publish a build folder. Build output holds the builder's home-folder paths and is too large to review. Add it to .gitignore.`,
    );
  }
}

function checkHistory(f, history) {
  for (const { email, count } of history ?? []) {
    if (emailIsFine(email)) continue;
    const at = email.lastIndexOf("@");
    f.warn(
      "history-email",
      "(git history)",
      `${count} commit${count === 1 ? "" : "s"} carry the address ${maskLocal(email.slice(0, at))}${email.slice(at)}. Commits cannot be edited after publishing; set a noreply address (git config user.email) and rewrite the history before the first push if that matters.`,
    );
  }
}

// ---------------------------------------------------------------------------
// The check itself: a pure function of a set of files
// ---------------------------------------------------------------------------

/**
 * @param {{files: Map<string,string>, history?: {email:string,count:number}[]}} repo
 * @param {{strictPins?: boolean}} [options]
 */
function check(repo, options = {}) {
  const f = new Findings();
  const { files } = repo;

  const parsed = new Map();
  const parse = (file) => {
    if (parsed.has(file)) return parsed.get(file);
    let value;
    try {
      value = JSON.parse(files.get(file));
    } catch (e) {
      f.fail("config-unreadable", file, `Not valid JSON (${e.message}), so it cannot be checked. A file that cannot be read is not a file that passes.`);
    }
    parsed.set(file, value);
    return value;
  };

  const frontend = [...files.keys()].filter(isFrontend);
  if (!files.has("index.html")) f.fail("missing-file", "index.html", "Not found, so the window's entry page cannot be checked.");
  if (frontend.length === 0) f.fail("missing-file", "src/", "No frontend files found. Is this the repository root?");

  checkFrontend(f, files);
  checkTauri(f, files, parse);

  if (files.has("package.json")) {
    const pkg = parse("package.json");
    if (pkg) checkPackage(f, pkg);
  } else {
    f.fail("missing-file", "package.json", "Not found, so install scripts cannot be checked.");
  }

  for (const [file, text] of files) {
    if (/^\.github\/workflows\/[^/]+\.ya?ml$/.test(file)) checkWorkflow(f, file, text, options.strictPins === true);
  }
  checkLeaks(f, files);
  checkBuildOutput(f, repo.buildOutput);
  checkHistory(f, repo.history);

  const failures = f.items.filter((i) => i.level === "fail");
  const warnings = f.items.filter((i) => i.level === "warn");
  const counted = (re) => [...files.keys()].filter((p) => re.test(p)).length;
  return {
    failures,
    warnings,
    allowed: f.allowed,
    stats: {
      files: files.size,
      frontend: frontend.length,
      workflows: counted(/^\.github\/workflows\//),
      capabilities: counted(/^src-tauri\/capabilities\//),
    },
  };
}

const RULE_IDS = [
  "no-html-injection", "no-dynamic-code", "no-network", "no-http-url", "no-external-asset", "dynamic-href",
  "csp-missing", "csp-unsafe-eval", "csp-wildcard", "csp-remote-source", "csp-unsafe-inline-script",
  "csp-weakened", "csp-unreviewed-source", "tauri-dangerous",
  "capability-denied-permission", "capability-core-default", "capability-unreviewed",
  "capability-remote", "capability-wildcard-window",
  "lifecycle-script", "non-registry-dependency",
  "workflow-pull-request-target", "workflow-expression-injection", "workflow-unpinned-action",
  "private-path", "private-email", "history-email", "build-output-not-ignored",
  "config-unreadable", "missing-file",
];

// ---------------------------------------------------------------------------
// Reading a repository from disk
// ---------------------------------------------------------------------------

/** Folders a build or a package manager writes. Thousands of generated files,
 *  and they carry the build machine's home folder. They are not read here; if
 *  git would publish them, that is the finding (build-output-not-ignored). */
const BUILD_DIRS = new Set(["node_modules", "dist", "dist-ssr", "target"]);
/** Without git there is no .gitignore to ask, so these are left out of the walk. */
const WALK_SKIPS = new Set([...BUILD_DIRS, ".git", ".claude"]);

function walk(root, dir = "", out = []) {
  for (const entry of readdirSync(join(root, dir), { withFileTypes: true })) {
    const rel = dir ? `${dir}/${entry.name}` : entry.name;
    if (entry.isDirectory()) {
      if (!WALK_SKIPS.has(entry.name)) walk(root, rel, out);
    } else if (entry.isFile()) {
      out.push(rel);
    }
  }
  return out;
}

/** The first build folder on the path, as "src-tauri/fuzz/target", or null. */
function buildDirOf(rel) {
  const parts = rel.split("/").slice(0, -1);
  const k = parts.findIndex((d) => BUILD_DIRS.has(d));
  return k === -1 ? null : parts.slice(0, k + 1).join("/");
}

/** execFileSync stops at 1 MB by default. A build folder git would publish is
 *  exactly the case that lists tens of thousands of paths. */
const GIT_OUTPUT_LIMIT = 512 * 1024 * 1024;

function listFiles(root) {
  try {
    // Tracked files plus new, not-yet-ignored ones, so a leak is caught
    // before the commit that would have published it.
    const out = execFileSync("git", ["-C", root, "ls-files", "-z", "--cached", "--others", "--exclude-standard"], {
      encoding: "utf8",
      maxBuffer: GIT_OUTPUT_LIMIT,
      stdio: ["ignore", "pipe", "ignore"],
    });
    return out.split("\0").filter(Boolean);
  } catch (e) {
    // No git, or not a repository: there is no .gitignore to ask, so walk the
    // folder. Any other failure must not quietly become a blinder check.
    if (e?.code === "ENOENT" || e?.status === 128) return walk(root);
    throw new Error(`could not list the files with git (${e?.code ?? e?.message ?? e})`);
  }
}

function loadRepo(root) {
  const files = new Map();
  const buildOutput = [];
  for (const rel of listFiles(root)) {
    if (buildDirOf(rel)) {
      buildOutput.push(rel);
      continue;
    }
    const path = join(root, rel);
    let buffer;
    try {
      if (!statSync(path).isFile()) continue;
      buffer = readFileSync(path);
    } catch {
      continue; // deleted from the working tree but still in the index
    }
    if (buffer.subarray(0, 8000).includes(0)) continue; // an image, a spreadsheet
    files.set(rel, buffer.toString("utf8"));
  }
  return { files, history: loadHistory(root), buildOutput };
}

function loadHistory(root) {
  try {
    const out = execFileSync("git", ["-C", root, "log", "--format=%ae%n%ce"], {
      encoding: "utf8",
      maxBuffer: GIT_OUTPUT_LIMIT,
      stdio: ["ignore", "pipe", "ignore"],
    });
    const counts = new Map();
    for (const email of out.split("\n").map((s) => s.trim()).filter(Boolean)) {
      counts.set(email, (counts.get(email) ?? 0) + 1);
    }
    // Author and committer are listed together; one commit counts once per role.
    return [...counts].map(([email, count]) => ({ email, count: Math.ceil(count / 2) }));
  } catch {
    return [];
  }
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

function print(result) {
  const { failures, warnings, allowed, stats } = result;
  const plural = (n, w) => `${n} ${w}${n === 1 ? "" : "s"}`;
  const covered = `${plural(stats.files, "file")} read: ${stats.frontend} frontend, ${plural(stats.workflows, "workflow")}, ${plural(stats.capabilities, "capability file")}`;
  const verdict = failures.length === 0 ? "Security check passed" : `Security check FAILED: ${plural(failures.length, "problem")}`;
  console.log(`${verdict}, ${plural(warnings.length, "warning")}, ${allowed.length} accepted. (${covered})\n`);

  const show = (mark, items) => {
    for (const i of items) {
      console.log(`${mark} [${i.rule}] ${i.file}${i.line ? `:${i.line}` : ""}`);
      console.log(`    ${i.message}`);
      if (i.snippet) console.log(`    > ${i.snippet}`);
    }
  };
  if (failures.length) {
    console.log("Problems to fix:");
    show("✗", failures);
    console.log("");
  }
  if (warnings.length) {
    console.log("Warnings:");
    show("!", warnings);
    console.log("");
  }
  if (allowed.length) {
    console.log("Accepted with a security-allow marker (review these):");
    for (const a of allowed) console.log(`- [${a.rule}] ${a.file}:${a.line}  ${a.reason}`);
    console.log("");
  }
}

// ---------------------------------------------------------------------------
// Self-test: small bad repositories, one mutation each
// ---------------------------------------------------------------------------

const SHA = "0123456789abcdef0123456789abcdef01234567";
const PROD_CSP =
  "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; base-uri 'self'; form-action 'none'; frame-ancestors 'none'";
const DEV_CSP = PROD_CSP.replace("script-src 'self'", "script-src 'self' 'unsafe-inline'").replace(
  "http://ipc.localhost",
  "http://ipc.localhost ws://localhost:1420 http://localhost:1420",
);

const tauriConf = (mutate = () => {}) => {
  const conf = {
    build: { devUrl: "http://localhost:1420", frontendDist: "../dist" },
    app: {
      withGlobalTauri: false,
      windows: [{ title: "Coin Heaven", width: 1440 }],
      security: { csp: PROD_CSP, devCsp: DEV_CSP, capabilities: ["default"] },
    },
  };
  mutate(conf);
  return JSON.stringify(conf);
};

const capability = (mutate = () => {}) => {
  const cap = {
    identifier: "default",
    windows: ["main"],
    permissions: ["core:default", "core:window:allow-start-dragging", "dialog:allow-open", "dialog:allow-save"],
  };
  mutate(cap);
  return JSON.stringify(cap);
};

const workflow = (...extra) =>
  [
    "name: CI",
    "on:",
    "  push:",
    "    branches: [main]",
    "  pull_request:",
    "jobs:",
    "  build:",
    "    runs-on: ubuntu-latest",
    "    steps:",
    `      - uses: actions/checkout@${SHA}`,
    "      - run: pnpm install --frozen-lockfile",
    ...extra,
  ].join("\n");

/** Addresses are assembled so this file passes its own leak check. */
const mail = (local, domain) => `${local}@${domain}`;

const GOOD = {
  "package.json": JSON.stringify({
    name: "fixture",
    scripts: { dev: "vite", build: "vite build" },
    dependencies: { react: "^19.2.0", "@tauri-apps/api": "^2.11.1" },
  }),
  "index.html": [
    "<!doctype html><html><head>",
    '<link rel="icon" type="image/png" href="/favicon.png" />',
    "</head><body>",
    '<script src="/diag.js"></script>',
    '<script type="module" src="/src/main.tsx"></script>',
    "</body></html>",
  ].join("\n"),
  "public/diag.js": "var mounted = root && root.innerHTML.length > 0;",
  "src/main.tsx": 'import "./index.css";',
  "src/index.css": "body { background: url(/bg.png); }",
  "src/lib/ipc.ts": 'export const OLLAMA = "http://localhost:11434"; // http://ipc.localhost',
  "src/lib/prices.ts": "export const refetch = () => fetch_prices(); // fetch_prices( is a command name",
  "src-tauri/tauri.conf.json": tauriConf(),
  "src-tauri/capabilities/default.json": capability(),
  ".github/workflows/ci.yml": workflow(),
  "README.md": `Questions: ${mail("noreply", "example.org")}. Clone with ${mail("git", "github.com")}:owner/repo.git. icons/128x128${mail("", "2x.png")}`,
};

/** [name, edits (null deletes), expected failing rules, expected warning rules] */
const CASES = [
  ["the baseline is clean", {}, [], []],

  // frontend
  ["dangerouslySetInnerHTML", { "src/a.tsx": "<div dangerouslySetInnerHTML={{ __html: x }} />" }, ["no-html-injection"], []],
  ["innerHTML assignment", { "src/a.ts": "el.innerHTML = value;" }, ["no-html-injection"], []],
  ["innerHTML append", { "src/a.ts": "el.innerHTML += value;" }, ["no-html-injection"], []],
  ["outerHTML via brackets", { "src/a.ts": 'el["outerHTML"] = value;' }, ["no-html-injection"], []],
  ["insertAdjacentHTML", { "src/a.ts": 'el.insertAdjacentHTML("beforeend", v);' }, ["no-html-injection"], []],
  ["reading innerHTML is fine", { "src/a.ts": "const n = el.innerHTML.length;" }, [], []],
  ["comparing innerHTML is fine", { "src/a.ts": 'if (el.innerHTML === "") {}' }, [], []],
  ["eval(", { "src/a.ts": "eval(code);" }, ["no-dynamic-code"], []],
  ["indirect eval", { "src/a.ts": "(0, eval)(code);" }, ["no-dynamic-code"], []],
  ["new Function(", { "src/a.ts": 'const f = new Function("a", "return a");' }, ["no-dynamic-code"], []],
  ["document.write(", { "src/a.ts": "document.write(html);" }, ["no-dynamic-code"], []],
  ["setTimeout with a string", { "src/a.ts": 'setTimeout("go()", 10);' }, ["no-dynamic-code"], []],
  ["setTimeout with a function is fine", { "src/a.ts": "setTimeout(() => go(), 10);" }, [], []],
  ["evaluate( is not eval(", { "src/a.ts": "evaluate(x); retrieval(y);" }, [], []],
  ["fetch(", { "src/a.ts": "const r = await fetch(url);" }, ["no-network"], []],
  ["XMLHttpRequest", { "src/a.ts": "const x = new XMLHttpRequest();" }, ["no-network"], []],
  ["WebSocket", { "src/a.ts": "const s = new WebSocket(addr);" }, ["no-network"], []],
  ["EventSource", { "src/a.ts": "const s = new EventSource(addr);" }, ["no-network"], []],
  ["sendBeacon", { "src/a.ts": "navigator.sendBeacon(addr, data);" }, ["no-network"], []],
  ["import from a URL", { "src/a.ts": 'const m = await import("https://cdn.example.com/x.js");' }, ["no-network"], []],
  ["fetch in public/", { "public/x.js": "fetch('/x');" }, ["no-network"], []],
  ["http:// to another host", { "src/a.ts": 'const u = "http://example.com/api";' }, ["no-http-url"], []],
  ["ws:// to another host", { "src/a.ts": 'const u = "ws://example.com/socket";' }, ["no-http-url"], []],
  ["localhost lookalike host", { "src/a.ts": 'const u = "http://localhost.evil.com/";' }, ["no-http-url"], []],
  // The part before "@" is the user name, so this is a request to attacker.io.
  ["localhost userinfo trick (it reads as an address too)", { "src/a.ts": `const u = "http://${mail("localhost", "attacker.io")}/";` }, ["no-http-url", "private-email"], []],
  ["host from a template", { "src/a.ts": "const u = `http://${host}/x`;" }, ["no-http-url"], []],
  ["http://127.0.0.1 and ::1 are fine", { "src/a.ts": 'const a = "http://127.0.0.1:8080", b = "http://[::1]:8080/";' }, [], []],
  ["https:// strings are CSP's job", { "src/a.ts": 'const u = "https://example.com/doc";' }, [], []],
  ["http:// in a test file is fine", { "src/a.test.ts": 'const u = "http://example.com/";' }, [], []],
  [
    "allow marker with a reason",
    { "src/a.ts": "// security-allow: no-network -- reads the local Ollama health page, reviewed in #12\nawait fetch(local);" },
    [],
    [],
  ],
  [
    "allow marker without a reason still fails",
    { "src/a.ts": "// security-allow: no-network\nawait fetch(local);" },
    ["no-network"],
    [],
  ],
  [
    "allow marker for another rule does not cover this one",
    { "src/a.ts": "// security-allow: no-dynamic-code -- unrelated reason here\nawait fetch(local);" },
    ["no-network"],
    [],
  ],
  [
    "external script in index.html",
    { "index.html": GOOD["index.html"].replace("</body>", '<script src="https://cdn.example.com/x.js"></script></body>') },
    ["no-external-asset"],
    [],
  ],
  [
    "protocol-relative stylesheet in index.html",
    { "index.html": GOOD["index.html"].replace("</head>", '<link rel="stylesheet" href="//fonts.example.com/css"></head>') },
    ["no-external-asset"],
    [],
  ],
  ["remote @import in CSS", { "src/index.css": '@import url("https://fonts.example.com/css?family=X");' }, ["no-external-asset"], []],
  ["remote url() in CSS", { "src/index.css": "a { background: url(https://example.com/x.png); }" }, ["no-external-asset"], []],
  ["link built from data", { "src/a.tsx": "<a href={row.sourceUrl}>x</a>" }, [], ["dynamic-href"]],
  ["literal links are fine", { "src/a.tsx": '<a href="#top">a</a><a href={"#x"}>b</a><a href={`#${id}`}>c</a>' }, [], []],
  [
    "dynamic link accepted with a reason",
    { "src/a.tsx": "{/* security-allow: dynamic-href -- scheme checked in the Rust command before storing */}\n<a href={row.sourceUrl}>x</a>" },
    [],
    [],
  ],

  // content policy
  ["no csp at all", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = null; }) }, ["csp-missing"], []],
  ["unsafe-eval in csp", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = PROD_CSP.replace("script-src 'self'", "script-src 'self' 'unsafe-eval'"); }) }, ["csp-unsafe-eval"], []],
  ["unsafe-eval in devCsp", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.devCsp = DEV_CSP.replace("script-src 'self'", "script-src 'self' 'unsafe-eval'"); }) }, ["csp-unsafe-eval"], []],
  ["wildcard in connect-src", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = PROD_CSP.replace("connect-src 'self'", "connect-src * 'self'"); }) }, ["csp-wildcard"], []],
  ["wildcard host in script-src", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = PROD_CSP.replace("script-src 'self'", "script-src 'self' https://*.example.com"); }) }, ["csp-wildcard"], []],
  ["https: in connect-src", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = PROD_CSP.replace("connect-src 'self'", "connect-src 'self' https:"); }) }, ["csp-remote-source"], []],
  ["http: in script-src", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = PROD_CSP.replace("script-src 'self'", "script-src 'self' http:"); }) }, ["csp-remote-source"], []],
  ["a named host in connect-src", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = PROD_CSP.replace("connect-src 'self'", "connect-src 'self' https://api.example.com"); }) }, ["csp-remote-source"], []],
  ["a remote image host", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = PROD_CSP.replace("img-src 'self'", "img-src 'self' https://img.example.com"); }) }, ["csp-remote-source"], []],
  ["a dev server in the production csp", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = PROD_CSP.replace("connect-src 'self'", "connect-src 'self' http://localhost:1420"); }) }, ["csp-remote-source"], []],
  ["unsafe-inline script in production", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = PROD_CSP.replace("script-src 'self'", "script-src 'self' 'unsafe-inline'"); }) }, ["csp-unsafe-inline-script"], []],
  ["data: in script-src", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = PROD_CSP.replace("script-src 'self'", "script-src 'self' data:"); }) }, ["csp-unreviewed-source"], []],
  ["object-src weakened", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = PROD_CSP.replace("object-src 'none'", "object-src 'self'"); }) }, ["csp-weakened"], []],
  ["frame-ancestors removed", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = PROD_CSP.replace("; frame-ancestors 'none'", ""); }) }, ["csp-weakened"], []],
  ["frame-ancestors opened", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = PROD_CSP.replace("frame-ancestors 'none'", "frame-ancestors https://example.com"); }) }, ["csp-weakened"], []],
  ["default-src missing", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = PROD_CSP.replace("default-src 'self'; ", ""); }) }, ["csp-weakened"], []],
  ["base-uri removed", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = PROD_CSP.replace("; base-uri 'self'", ""); }) }, ["csp-weakened"], []],
  ["csp written as an object", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.csp = { "default-src": "'self'", "connect-src": ["'self'", "https://api.example.com"], "object-src": "'none'", "base-uri": "'self'", "form-action": "'none'", "frame-ancestors": "'none'" }; }) }, ["csp-remote-source"], []],
  ["a platform file weakens the csp", { "src-tauri/tauri.windows.conf.json": JSON.stringify({ app: { security: { csp: PROD_CSP.replace("script-src 'self'", "script-src 'self' 'unsafe-eval'") } } }) }, ["csp-unsafe-eval"], []],
  ["withGlobalTauri", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.withGlobalTauri = true; }) }, ["tauri-dangerous"], []],
  ["dangerous asset csp switch", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.dangerousDisableAssetCspModification = true; }) }, ["tauri-dangerous"], []],
  ["asset protocol with a wildcard scope", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.assetProtocol = { enable: true, scope: ["**"] }; }) }, ["tauri-dangerous"], []],
  ["a window that loads a website", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.windows[0].url = "https://example.com/app"; }) }, ["tauri-dangerous"], []],
  ["unparseable tauri.conf.json", { "src-tauri/tauri.conf.json": "{ not json" }, ["config-unreadable"], []],

  // capabilities
  ["fs permission", { "src-tauri/capabilities/default.json": capability((c) => { c.permissions.push("fs:default"); }) }, ["capability-denied-permission"], []],
  ["shell permission", { "src-tauri/capabilities/default.json": capability((c) => { c.permissions.push("shell:allow-open"); }) }, ["capability-denied-permission"], []],
  ["http permission", { "src-tauri/capabilities/default.json": capability((c) => { c.permissions.push("http:default"); }) }, ["capability-denied-permission"], []],
  ["process permission", { "src-tauri/capabilities/default.json": capability((c) => { c.permissions.push("process:allow-exit"); }) }, ["capability-denied-permission"], []],
  ["os permission", { "src-tauri/capabilities/default.json": capability((c) => { c.permissions.push("os:default"); }) }, ["capability-denied-permission"], []],
  ["opener without a scope", { "src-tauri/capabilities/default.json": capability((c) => { c.permissions.push("opener:default"); }) }, ["capability-denied-permission"], []],
  ["opener with a wildcard scope", { "src-tauri/capabilities/default.json": capability((c) => { c.permissions.push({ identifier: "opener:allow-open-url", allow: [{ url: "https://*" }] }); }) }, ["capability-denied-permission"], []],
  ["opener with a narrow scope is still unreviewed", { "src-tauri/capabilities/default.json": capability((c) => { c.permissions.push({ identifier: "opener:allow-open-url", allow: [{ url: "https://github.com/owner/repo/releases/*" }] }); }) }, ["capability-unreviewed"], []],
  ["core:default removed", { "src-tauri/capabilities/default.json": capability((c) => { c.permissions = c.permissions.filter((p) => p !== "core:default"); }) }, ["capability-core-default"], []],
  [
    "core:default swapped for a broader core permission",
    { "src-tauri/capabilities/default.json": capability((c) => { c.permissions = ["core:window:allow-create", "dialog:allow-open"]; }) },
    ["capability-core-default", "capability-unreviewed"],
    [],
  ],
  ["a new window permission", { "src-tauri/capabilities/default.json": capability((c) => { c.permissions.push("core:webview:allow-create-webview-window"); }) }, ["capability-unreviewed"], []],
  ["remote capability", { "src-tauri/capabilities/default.json": capability((c) => { c.remote = { urls: ["https://*.example.com"] }; }) }, ["capability-remote"], []],
  ["capability for every window", { "src-tauri/capabilities/default.json": capability((c) => { c.windows = ["*"]; }) }, ["capability-wildcard-window"], []],
  ["a second capability file", { "src-tauri/capabilities/extra.json": capability((c) => { c.identifier = "extra"; c.permissions.push("fs:allow-read-file"); }) }, ["capability-denied-permission"], []],
  ["an inline capability", { "src-tauri/tauri.conf.json": tauriConf((c) => { c.app.security.capabilities.push({ identifier: "inline", windows: ["main"], permissions: ["core:default", "shell:default"] }); }) }, ["capability-denied-permission"], []],
  ["no capability files", { "src-tauri/capabilities/default.json": null }, ["missing-file"], []],
  ["a TOML capability cannot be read", { "src-tauri/capabilities/extra.toml": 'identifier = "extra"' }, ["config-unreadable"], []],

  // supply chain
  ["postinstall script", { "package.json": JSON.stringify({ scripts: { postinstall: "node x.js" } }) }, ["lifecycle-script"], []],
  ["preinstall script", { "package.json": JSON.stringify({ scripts: { preinstall: "node x.js" } }) }, ["lifecycle-script"], []],
  ["install script", { "package.json": JSON.stringify({ scripts: { install: "node-gyp rebuild" } }) }, ["lifecycle-script"], []],
  ["prepare script", { "package.json": JSON.stringify({ scripts: { prepare: "husky" } }) }, ["lifecycle-script"], []],
  ["a script merely named like one is fine", { "package.json": JSON.stringify({ scripts: { "check:install": "node x.js", installer: "x" } }) }, [], []],
  ["a dependency from git", { "package.json": JSON.stringify({ dependencies: { thing: "github:owner/thing#main" } }) }, ["non-registry-dependency"], []],
  ["a dependency from a tarball URL", { "package.json": JSON.stringify({ devDependencies: { thing: "https://example.com/thing.tgz" } }) }, ["non-registry-dependency"], []],
  ["package.json is not JSON", { "package.json": "{ nope" }, ["config-unreadable"], []],
  ["pull_request_target", { ".github/workflows/ci.yml": workflow().replace("  pull_request:", "  pull_request_target:") }, ["workflow-pull-request-target"], []],
  ["pull_request_target in a list", { ".github/workflows/ci.yml": workflow().replace("on:\n  push:\n    branches: [main]\n  pull_request:", "on: [push, pull_request_target]") }, ["workflow-pull-request-target"], []],
  ["pull_request_target in a comment is fine", { ".github/workflows/ci.yml": `# never use pull_request_target here\n${workflow()}` }, [], []],
  ["github.event text in a one-line run", { ".github/workflows/ci.yml": workflow('      - run: echo "${{ github.event.issue.title }}"') }, ["workflow-expression-injection"], []],
  [
    "github.event text in a block run",
    { ".github/workflows/ci.yml": workflow("      - name: Greet", "        run: |", "          echo hello", '          echo "${{ github.event.pull_request.body }}"') },
    ["workflow-expression-injection"],
    [],
  ],
  ["github.head_ref in a run", { ".github/workflows/ci.yml": workflow("      - run: git checkout ${{ github.head_ref }}") }, ["workflow-expression-injection"], []],
  [
    "github.event through env is fine",
    { ".github/workflows/ci.yml": workflow("      - name: Greet", "        env:", "          TITLE: ${{ github.event.issue.title }}", "        run: echo \"$TITLE\"") },
    [],
    [],
  ],
  ["github.event_name is not github.event.*", { ".github/workflows/ci.yml": workflow('      - run: echo "${{ github.event_name }}"') }, [], []],
  ["an action on a floating tag", { ".github/workflows/ci.yml": workflow("      - uses: actions/setup-node@v4") }, [], ["workflow-unpinned-action"]],
  ["a local action is fine", { ".github/workflows/ci.yml": workflow("      - uses: ./.github/actions/setup") }, [], []],
  ["an action with a short sha is a floating ref", { ".github/workflows/ci.yml": workflow("      - uses: actions/setup-node@0123456") }, [], ["workflow-unpinned-action"]],

  // leaks
  ["a person's home folder", { "docs/a.md": `Data is under ${MAC_HOME}jdoe/Documents/x` }, ["private-path"], []],
  ["a Windows home folder", { "docs/a.md": `See C:${WIN_HOME}jdoe\\AppData` }, ["private-path"], []],
  ["a placeholder home folder is fine", { "docs/a.md": `Use ${MAC_HOME}<you>/Library or ${MAC_HOME}Shared or C:${WIN_HOME}%USERNAME%\\x` }, [], []],
  ["a personal email", { "docs/a.md": `Write to ${mail("jane.doe", "gmail.com")}` }, ["private-email"], []],
  ["a personal email in source", { "src/a.ts": `// ${mail("jane.doe", "company.io")}` }, ["private-email"], []],
  ["noreply addresses are fine", { "docs/a.md": `${mail("12345+jane", "users.noreply.github.com")}, ${mail("no-reply", "company.io")}` }, [], []],
  ["example addresses are fine", { "src-tauri/src/pii.rs": `mask("${mail("ana", "example.com")}")` }, [], []],
  ["lockfile versions are not emails", { "pnpm-lock.yaml": `  '@types/node@26.4.1': x\n  ${mail("who", "real-person.com")}: z` }, [], []],

  // build output that git would publish
  [
    "a build folder that is not ignored",
    { __buildOutput: ["src-tauri/fuzz/target/debug/a.d", "src-tauri/fuzz/target/debug/b.d", "node_modules/x/index.js"] },
    ["build-output-not-ignored"],
    [],
  ],

  // inline script in the entry page
  ["fetch in an inline script", { "index.html": GOOD["index.html"].replace("</body>", "<script>\nfetch('/x');\n</script></body>") }, ["no-network"], []],
  [
    "eval in an inline module script",
    { "index.html": GOOD["index.html"].replace("</body>", '<script type="module">eval(window.name)</script></body>') },
    ["no-dynamic-code"],
    [],
  ],
  [
    "innerHTML assigned in an inline script",
    { "index.html": GOOD["index.html"].replace("</body>", "<script>document.body.innerHTML = location.hash;</script></body>") },
    ["no-html-injection"],
    [],
  ],
  [
    "a JSON data block is not code",
    { "index.html": GOOD["index.html"].replace("</body>", '<script type="application/json">{"a":"fetch(x)"}</script></body>') },
    [],
    [],
  ],
  [
    "words in the page text are not code",
    { "index.html": GOOD["index.html"].replace("</body>", "<p>we never call fetch( or eval( here</p></body>") },
    [],
    [],
  ],

  // history
  ["a personal address in the history", { __history: [{ email: mail("jane", "company.io"), count: 3 }] }, [], ["history-email"]],
  ["a noreply address in the history is fine", { __history: [{ email: mail("12345+jane", "users.noreply.github.com"), count: 3 }] }, [], []],

  // the check itself
  ["missing index.html", { "index.html": null }, ["missing-file"], []],
  ["an empty frontend", { "src/main.tsx": null, "src/index.css": null, "src/lib/ipc.ts": null, "src/lib/prices.ts": null, "public/diag.js": null }, ["missing-file"], []],
];

/** "ok", "skipped" (no git on this machine) or a sentence saying what broke. */
function loaderCase() {
  const git = (cwd, args, input) =>
    execFileSync("git", args, { cwd, input, encoding: "utf8", stdio: ["pipe", "pipe", "ignore"] });
  try {
    git(tmpdir(), ["--version"]);
  } catch {
    return "skipped";
  }
  const dir = mkdtempSync(join(tmpdir(), "check-security-"));
  try {
    for (const [path, content] of Object.entries(GOOD)) {
      mkdirSync(dirname(join(dir, path)), { recursive: true });
      writeFileSync(join(dir, path), content);
    }
    git(dir, ["init", "-q"]);
    // Index entries only: no files on disk, but git lists them like any other.
    // 12,000 paths of about 110 bytes is well past the 1 MB pipe default.
    const empty = git(dir, ["hash-object", "-w", "--stdin"], "").trim();
    const entries = [];
    for (let n = 0; n < 12000; n++) {
      entries.push(`100644 ${empty} 0\tsrc-tauri/fuzz/target/release/build/crate-${String(n).padStart(6, "0")}/out/${"x".repeat(40)}.d`);
    }
    git(dir, ["update-index", "--add", "--index-info"], entries.join("\n") + "\n");

    const result = check(loadRepo(dir));
    const rules = [...new Set(result.failures.map((i) => i.rule))];
    if (rules.length !== 1 || rules[0] !== "build-output-not-ignored") {
      return `expected only build-output-not-ignored, got [${rules}]`;
    }
    if (!/12000 files/.test(result.failures[0].message)) return `counted the wrong number: ${result.failures[0].message.slice(0, 40)}`;
    return "ok";
  } catch (e) {
    return `the loader threw: ${e.message}`;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function selfTest(verbose) {
  let bad = 0;
  const failedRules = new Set();
  const warnedRules = new Set();
  // Passing lines are noise inside `pnpm verify`; a failure is always printed.
  const pass = (text) => verbose && console.log(`✓ ${text}`);

  for (const [name, edits, wantFail, wantWarn] of CASES) {
    const files = new Map(Object.entries(GOOD));
    let history;
    let buildOutput;
    for (const [path, content] of Object.entries(edits)) {
      if (path === "__history") history = content;
      else if (path === "__buildOutput") buildOutput = content;
      else if (content === null) files.delete(path);
      else files.set(path, content);
    }
    const result = check({ files, history, buildOutput });
    const gotFail = [...new Set(result.failures.map((i) => i.rule))].sort();
    const gotWarn = [...new Set(result.warnings.map((i) => i.rule))].sort();
    gotFail.forEach((r) => failedRules.add(r));
    gotWarn.forEach((r) => warnedRules.add(r));
    const ok = JSON.stringify(gotFail) === JSON.stringify([...wantFail].sort()) && JSON.stringify(gotWarn) === JSON.stringify([...wantWarn].sort());
    if (ok) {
      pass(name);
    } else {
      bad++;
      console.log(`✗ ${name}`);
      console.log(`    expected fail [${wantFail}] warn [${wantWarn}]`);
      console.log(`    got      fail [${gotFail}] warn [${gotWarn}]`);
    }
  }

  // Unpinned actions are a warning now and must become a failure on demand.
  const pinned = check(
    { files: new Map(Object.entries({ ...GOOD, ".github/workflows/ci.yml": workflow("      - uses: actions/setup-node@v4") })) },
    { strictPins: true },
  );
  const strictOk = pinned.failures.length === 1 && pinned.failures[0].rule === "workflow-unpinned-action";
  if (strictOk) pass("--strict-pins turns an unpinned action into a failure");
  else {
    bad++;
    console.log("✗ --strict-pins turns an unpinned action into a failure");
  }

  // The accepted line has to be reported, or a review cannot see it.
  const accepted = check({
    files: new Map(Object.entries({ ...GOOD, "src/a.ts": "// security-allow: no-network -- reviewed local health probe\nawait fetch(local);" })),
  });
  const listedOk = accepted.allowed.length === 1 && accepted.allowed[0].rule === "no-network" && accepted.allowed[0].line === 2;
  if (listedOk) pass("an accepted line is listed in the output");
  else {
    bad++;
    console.log("✗ an accepted line is listed in the output");
  }

  // The loader, against a real repository. This is the case that once made the
  // check go quietly blind: a build folder with more paths than a pipe buffer.
  const loaded = loaderCase();
  if (loaded === "skipped") {
    console.log("- the loader case was skipped: git is not installed");
  } else if (loaded === "ok") {
    pass("a build folder with thousands of paths is still found");
  } else {
    bad++;
    console.log("✗ a build folder with thousands of paths is still found");
    console.log(`    ${loaded}`);
  }

  // Every rule must be exercised, so a rule added without a fixture fails here.
  const seen = new Set([...failedRules, ...warnedRules]);
  const unexercised = RULE_IDS.filter((id) => !seen.has(id));
  const unknown = [...seen].filter((id) => !RULE_IDS.includes(id));
  if (unexercised.length || unknown.length) {
    bad++;
    console.log(`✗ every rule has a fixture`);
    if (unexercised.length) console.log(`    no fixture triggers: ${unexercised.join(", ")}`);
    if (unknown.length) console.log(`    not in RULE_IDS: ${unknown.join(", ")}`);
  } else {
    pass(`every rule (${RULE_IDS.length}) is triggered by at least one fixture`);
  }

  console.log(bad === 0 ? `\nSelf-test passed: ${CASES.length} cases.` : `\nSelf-test FAILED: ${bad} problem(s).`);
  return bad === 0;
}

// ---------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------

function main() {
  const args = process.argv.slice(2);
  if (args.includes("--help") || args.includes("-h")) {
    console.log("Usage: node scripts/check-security.mjs [--self-test [--verbose]] [--strict-pins] [--root <dir>]");
    return 0;
  }
  if (args.includes("--self-test")) return selfTest(args.includes("--verbose")) ? 0 : 1;

  const rootFlag = args.indexOf("--root");
  const root = resolve(
    rootFlag !== -1 && args[rootFlag + 1] ? args[rootFlag + 1] : join(dirname(fileURLToPath(import.meta.url)), ".."),
  );
  let result;
  try {
    result = check(loadRepo(root), { strictPins: args.includes("--strict-pins") });
  } catch (e) {
    // A check that could not run is not a check that passed.
    console.log(`Security check FAILED: it could not read the repository: ${e.message}`);
    return 1;
  }
  print(result);
  return result.failures.length === 0 ? 0 : 1;
}

process.exitCode = main();
