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
const taskSelector = "[data-task-transition-id]";
const visibleTasks = new Set();
const namedRows = [];
const nameTaskRow = (row) => {
    row.style.viewTransitionName = `home-card-task-${row.dataset.taskTransitionId}`;
    namedRows.push(row);
};

root.classList.add(activeClass);
try {
    if (page === "todos") {
        const top = Math.max(0, bounds.top + card.clientTop);
        const bottom = Math.min(root.clientHeight, bounds.top + card.clientTop + card.clientHeight);
        // Rows clipped by the preview's scrollport would pop out of the card.
        const visibleRows = Array.from(card.querySelectorAll(taskSelector)).filter((row) => {
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

            const { left, top } = destination.getBoundingClientRect();
            const radius = Math.hypot(
                Math.max(Math.abs(centerX), Math.abs(root.clientWidth - centerX)),
                Math.max(Math.abs(centerY), Math.abs(root.clientHeight - centerY)),
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
    root.classList.remove(activeClass);
    for (const property of properties) root.style.removeProperty(property);
    for (const row of namedRows) row.style.removeProperty("view-transition-name");
}
