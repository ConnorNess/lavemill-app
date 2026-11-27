const page = await dioxus.recv();
const root = document.documentElement;
const activeClass = "home-card-transition";

if (root.classList.contains(activeClass)) {
    dioxus.send(false);
    return;
}

const card = document.querySelector(`[data-card-page="${CSS.escape(page)}"]`)
    ?.closest(".dashboard-card");
if (!card || !document.startViewTransition ||
    matchMedia("(prefers-reduced-motion: reduce)").matches) {
    dioxus.send(true);
    return;
}

const bounds = card.getBoundingClientRect();
const centerX = bounds.left + bounds.width / 2;
const centerY = bounds.top + bounds.height / 2;
const destinationSelector = `#main.${CSS.escape(page)}-page`;
const properties = ["--card-reveal-x", "--card-reveal-y", "--card-reveal-radius"];

root.classList.add(activeClass);
try {
    const transition = document.startViewTransition(() => new Promise((resolve) => {
        const finish = () => {
            observer.disconnect();
            clearTimeout(timeout);
            resolve();
        };
        const findPage = () => {
            const destination = document.querySelector(destinationSelector);
            if (!destination) return;

            const { left, top } = destination.getBoundingClientRect();
            const radius = Math.hypot(
                Math.max(Math.abs(centerX), Math.abs(root.clientWidth - centerX)),
                Math.max(Math.abs(centerY), Math.abs(root.clientHeight - centerY)),
            );
            root.style.setProperty("--card-reveal-x", `${centerX - left}px`);
            root.style.setProperty("--card-reveal-y", `${centerY - top}px`);
            root.style.setProperty("--card-reveal-radius", `${radius}px`);
            finish();
        };
        const observer = new MutationObserver(findPage);
        observer.observe(document.body, { childList: true, subtree: true });
        const timeout = setTimeout(() => {
            transition.skipTransition();
            finish();
        }, 450);
        dioxus.send(true);
    }));
    transition.ready.catch(() => {});
    await transition.finished;
} finally {
    root.classList.remove(activeClass);
    for (const property of properties) root.style.removeProperty(property);
}
