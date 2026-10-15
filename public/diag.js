// Startup watchdog.
//
// A webview that fails during module initialisation renders nothing and says
// nothing — the window is simply blank, with no way to tell a crashed bundle
// from a slow one. This reports to the Rust side, which logs to stderr, but
// only when something has actually gone wrong: a healthy launch is silent.
(function () {
  var errs = [];
  addEventListener("error", function (e) {
    errs.push(
      "error: " + (e.message || e.type) +
      " @ " + (e.filename || "?") + ":" + (e.lineno || 0)
    );
  }, true);
  addEventListener("unhandledrejection", function (e) {
    errs.push("rejection: " + String(e && e.reason));
  });

  setTimeout(function () {
    var root = document.getElementById("root");
    var mounted = root && root.innerHTML.length > 0;
    if (mounted && errs.length === 0) return; // all well; stay quiet

    try {
      window.__TAURI_INTERNALS__.invoke("js_diagnostic", {
        payload: JSON.stringify({
          mounted: !!mounted,
          objectFrozen: Object.isFrozen(Object.prototype),
          readyState: document.readyState,
          errors: errs.slice(0, 8),
        }),
      });
    } catch (e) {
      // If IPC itself is unavailable there is nowhere left to report.
    }
  }, 4000);
})();
