// Runs in the page: the element map. One entry per element worth pointing at (a component's root
// element, a shadcn `data-slot`, or something interactive), with its box, component chain (from
// React fibers), source location (`data-src`, added by @charet/sdk/vite), selector, tag and text.
(() => {
  // Wrappers that say nothing about the UI: routers, providers, Radix internals.
  const SKIP =
    /^(RenderedRoute|Routes?|RouterProvider|DataRoutes|Outlet|Navigate|MemoryRouter|Router|.*Provider|.*Context|StrictMode|Suspense|Fragment|.*ErrorBoundary|Slot|SlotClone|Primitive.*|Presence|Portal)$/;
  const nameOf = (type) => {
    if (typeof type === "function") return type.displayName || type.name || "";
    if (type && typeof type === "object")
      return type.displayName || nameOf(type.render) || nameOf(type.type);
    return "";
  };
  const isComponent = (fiber) =>
    typeof fiber.type === "function" ||
    (!!fiber.type &&
      typeof fiber.type === "object" &&
      ("render" in fiber.type || "type" in fiber.type));
  const isHost = (fiber) => typeof fiber.type === "string";
  const selectorOf = (el) => {
    const parts = [];
    for (let e = el; e && e !== document.body; e = e.parentElement) {
      let i = 1;
      for (let s = e.previousElementSibling; s; s = s.previousElementSibling)
        if (s.tagName === e.tagName) i++;
      parts.unshift(`${e.tagName.toLowerCase()}:nth-of-type(${i})`);
    }
    return parts.length ? `body > ${parts.join(" > ")}` : "body";
  };
  const vw = innerWidth;
  const vh = innerHeight;
  const out = [];
  for (const el of document.body.querySelectorAll("*")) {
    const key = Object.keys(el).find((k) => k.startsWith("__reactFiber$"));
    if (!key) continue;
    const fiber = el[key];
    let parent = fiber.return;
    while (parent && !isHost(parent) && !isComponent(parent)) parent = parent.return;
    const componentRoot = !!parent && isComponent(parent);
    const slot = el.getAttribute("data-slot") ?? undefined;
    const interactive = el.matches(
      "button,a[href],input,select,textarea,label,[role=button],[role=tab],[role=menuitem],[role=checkbox],[role=switch],[role=row]",
    );
    if (!componentRoot && !slot && !interactive) continue;
    const r = el.getBoundingClientRect();
    const x = Math.max(0, r.left);
    const y = Math.max(0, r.top);
    const w = Math.min(vw, r.right) - x;
    const h = Math.min(vh, r.bottom) - y;
    if (w < 4 || h < 4) continue;
    const style = getComputedStyle(el);
    if (style.visibility === "hidden" || Number(style.opacity) === 0) continue;
    const chain = [];
    for (let q = fiber.return; q; q = q.return) {
      if (!isComponent(q)) continue;
      const name = nameOf(q.type);
      if (name && /^[A-Z]/.test(name) && !SKIP.test(name) && chain[0] !== name) chain.unshift(name);
    }
    const text = (el.innerText || el.value || el.getAttribute("aria-label") || "")
      .replace(/\s+/g, " ")
      .trim()
      .slice(0, 80);
    const src = el.getAttribute("data-src") ?? undefined;
    out.push({
      x: Math.round(x),
      y: Math.round(y),
      w: Math.round(w),
      h: Math.round(h),
      chain: chain.slice(-8),
      sel: selectorOf(el),
      tag: el.tagName.toLowerCase(),
      text,
      ...(src ? { src } : {}),
      ...(slot ? { slot } : {}),
    });
  }
  return out;
})();
