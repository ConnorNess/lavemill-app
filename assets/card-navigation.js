const [page, returningHome] = await dioxus.recv();
const root = document.documentElement;
const activeClass = "home-card-transition";
const returnClass = "home-card-return";

if (root.classList.contains(activeClass)) {
    dioxus.send(false);
    return;
}

const pageKey = CSS.escape(page);
const findCard = () => document.querySelector(`[data-card-page="${pageKey}"]`)
    ?.closest(".dashboard-card");
const source = returningHome ? document.querySelector("main.interior-page") : findCard();
if (!source || !document.startViewTransition ||
    matchMedia("(prefers-reduced-motion: reduce)").matches) {
    dioxus.send(true);
    return;
}

const bounds = source.getBoundingClientRect();
const viewportWidth = root.clientWidth;
const viewportHeight = root.clientHeight;
const destinationSelector = returningHome ? "main.home-dashboard" : `main#main.${pageKey}-page`;
const properties = ["--card-reveal-x", "--card-reveal-y", "--card-reveal-radius"];
const taskSelector = "[data-task-transition-id]";
const visibleTasks = new Set();
const namedRows = [];
const nameTaskRow = (row) => {
    row.style.viewTransitionName = `home-card-task-${row.dataset.taskTransitionId}`;
    namedRows.push(row);
};

root.classList.add(activeClass);
root.classList.toggle(returnClass, returningHome);
try {
    if (page === "todos" && !returningHome) {
        const top = Math.max(0, bounds.top + source.clientTop);
        const bottom = Math.min(viewportHeight, bounds.top + source.clientTop + source.clientHeight);
        // Rows clipped by the preview's scrollport would pop out of the card.
        const visibleRows = Array.from(source.querySelectorAll(taskSelector)).filter((row) => {
            const bounds = row.getBoundingClientRect();
            return bounds.height > 0 && bounds.top >= top && bounds.bottom <= bottom;
        });
        for (const row of visibleRows) {
            visibleTasks.add(row.dataset.taskTransitionId);
            nameTaskRow(row);
        }
    }

    const transition = document.startViewTransition(() => new Promise((resolve) => {
        const finish = () => {
            observer.disconnect();
            clearTimeout(timeout);
            resolve();
        };
        const findPage = () => {
            const destination = document.querySelector(destinationSelector);
            if (!destination) return;

            let anchorBounds = bounds;
            if (returningHome) {
                const card = findCard();
                const anchor = card ?? document.querySelector(`[data-home-return-anchor="${pageKey}"]`);
                if (!anchor) return;
                card?.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" });
                anchorBounds = anchor.getBoundingClientRect();
            }
            const { left, top } = returningHome ? bounds : destination.getBoundingClientRect();
            const centerX = anchorBounds.left + anchorBounds.width / 2;
            const centerY = anchorBounds.top + anchorBounds.height / 2;
            const radius = Math.hypot(
                Math.max(Math.abs(centerX), Math.abs((returningHome ? viewportWidth : root.clientWidth) - centerX)),
                Math.max(Math.abs(centerY), Math.abs((returningHome ? viewportHeight : root.clientHeight) - centerY)),
            );
            root.style.setProperty("--card-reveal-x", `${centerX - left}px`);
            root.style.setProperty("--card-reveal-y", `${centerY - top}px`);
            root.style.setProperty("--card-reveal-radius", `${radius}px`);
            if (visibleTasks.size) {
                for (const row of destination.querySelectorAll(taskSelector)) {
                    if (visibleTasks.has(row.dataset.taskTransitionId)) nameTaskRow(row);
                }
            }
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
    root.classList.remove(activeClass, returnClass);
    for (const property of properties) root.style.removeProperty(property);
    for (const row of namedRows) row.style.removeProperty("view-transition-name");
}
