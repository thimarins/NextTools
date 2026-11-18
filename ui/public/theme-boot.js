(function () {
  try {
    var raw = localStorage.getItem("nexttool.settings");
    var s = raw ? JSON.parse(raw) : {};
    var theme = s.theme || "system";
    var dark = theme === "dark" || (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
    var el = document.documentElement;
    if (dark) el.classList.add("dark");
    if (s.accent) el.style.setProperty("--hue", String(s.accent));
    if (s.density) el.setAttribute("data-density", s.density);
    if (s.font_scale) el.style.fontSize = s.font_scale + "%";
  } catch (e) {
    /* storage unavailable: defaults */
  }
})();
