// URL-selected presentation shares the existing engine, pack and controls.
export function setupPresentation() {
  const root = document.documentElement;
  const params = new URLSearchParams(location.search);
  const choice = document.querySelector("#theme-choice");
  const applyTheme = (theme) => {
    if (theme === "auto") delete root.dataset.theme;
    else root.dataset.theme = theme;
    if (theme === "intigraph" && !document.querySelector("#intigraph-fonts")) {
      const fonts = document.createElement("link");
      fonts.id = "intigraph-fonts";
      fonts.rel = "stylesheet";
      fonts.href = "https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap";
      document.head.append(fonts);
    }
  };
  const theme = params.get("theme");
  if ([...choice.options].some((option) => option.value === theme)) choice.value = theme;
  applyTheme(choice.value);
  choice.addEventListener("change", () => applyTheme(choice.value));
  const embed = params.get("embed") === "1";
  if (embed) {
    root.dataset.embed = "true";
    if (window.parent !== window) {
      let lastHeight = 0;
      const reportHeight = () => {
        // Content height, not viewport height: a parent resizing the iframe
        // must be able to shrink it as well as grow it when tabs change.
        const height = Math.ceil(document.body.getBoundingClientRect().height);
        if (height === lastHeight) return;
        lastHeight = height;
        window.parent.postMessage({ type: "quipu:resize", height }, "*");
      };
      new ResizeObserver(reportHeight).observe(document.body);
    }
  }
  return embed ? "map" : "ask";
}
