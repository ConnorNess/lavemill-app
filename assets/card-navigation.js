const [page, returningHome, recipeId] = await dioxus.recv();
const root = document.documentElement;
const activeClass = "home-card-transition";
const returnClass = "home-card-return";
const shoppingClass = "home-card-open-shopping";

if (root.classList.contains(activeClass)) {
    dioxus.send(false);
    return;
}

const pageKey = CSS.escape(page);
const recipeKey = recipeId == null ? null : CSS.escape(recipeId);
const openingRecipeEditor = page === "recipe-edit";
const findCard = () => document.querySelector(`[data-card-page="${pageKey}"]`)
    ?.closest(".dashboard-card");
const findHeaderAnchor = () => document.querySelector(`[data-home-return-anchor="${pageKey}"]`);
const openingFromHeader = !returningHome && recipeKey === null && (page === "users" || page === "recipes");
const interiorPage = document.querySelector("main#main.interior-page");
let source;
if (returningHome) {
    source = interiorPage;
} else if (recipeKey !== null) {
    const anchor = openingRecipeEditor ? "data-recipe-edit-link-id" : "data-recipe-card-id";
    source = document.querySelector(`[${anchor}="${recipeKey}"]`);
} else {
    source = openingFromHeader ? findHeaderAnchor() : findCard();
}
if (!source || !document.startViewTransition ||
    matchMedia("(prefers-reduced-motion: reduce)").matches) {
    dioxus.send(true);
    return;
}

const bounds = source.getBoundingClientRect();
const viewportWidth = root.clientWidth;
const viewportHeight = root.clientHeight;
let destinationPage = `.${pageKey}-page`;
if (recipeKey !== null) {
    const destination = openingRecipeEditor ? "data-recipe-edit-id" : "data-recipe-detail-id";
    destinationPage = `[${destination}="${recipeKey}"]`;
} else if (page === "recipes") {
    destinationPage = "[data-recipes-index]";
}
const destinationSelector = returningHome ? "main.home-dashboard" : `main#main${destinationPage}`;
const properties = ["--card-reveal-x", "--card-reveal-y", "--card-reveal-radius"];
const taskSelector = "[data-task-transition-id]";
const visibleTasks = new Set();
const namedRows = [];
const visibleTaskRows = (container, clippingCard) => {
    const cardTop = clippingCard
        ? clippingCard.getBoundingClientRect().top + clippingCard.clientTop
        : 0;
    const top = Math.max(0, cardTop);
    const bottom = clippingCard
        ? Math.min(root.clientHeight, cardTop + clippingCard.clientHeight)
        : root.clientHeight;
    // Clipped rows would pop into view when promoted to their own snapshot.
    return Array.from(container.querySelectorAll(taskSelector)).filter((row) => {
        const { height, top: rowTop, bottom: rowBottom } = row.getBoundingClientRect();
        return height > 0 && rowTop >= top && rowBottom <= bottom;
    });
};
const nameTaskRow = (row) => {
    row.style.viewTransitionName = `home-card-task-${row.dataset.taskTransitionId}`;
    namedRows.push(row);
};
const outgoingPage = openingFromHeader || recipeKey !== null ? interiorPage : null;
const outgoingClass = "home-card-source";
const openingShopping = page === "shopping" && !returningHome;

root.classList.add(activeClass);
root.classList.toggle(returnClass, returningHome);
root.classList.toggle(shoppingClass, openingShopping);
try {
    outgoingPage?.classList.add(outgoingClass);
    if (page === "todos") {
        for (const row of visibleTaskRows(source, returningHome ? null : source)) {
            visibleTasks.add(row.dataset.taskTransitionId);
            nameTaskRow(row);
        }
    }

    const transition = document.startViewTransition(() => new Promise((resolve) => {
        outgoingPage?.classList.remove(outgoingClass);
        const finish = () => {
            observer.disconnect();
            clearTimeout(timeout);
            resolve();
        };
        const findPage = () => {
            const destination = document.querySelector(destinationSelector);
            if (!destination) return;
            if (openingShopping) {
                finish();
                return;
            }

            let anchorBounds = bounds;
            let landingCard;
            if (returningHome) {
                landingCard = findCard();
                const anchor = landingCard ?? findHeaderAnchor();
                if (!anchor) return;
                landingCard?.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" });
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
                const destinationRows = landingCard
                    ? visibleTaskRows(landingCard, landingCard)
                    : destination.querySelectorAll(taskSelector);
                for (const row of destinationRows) {
                    if (visibleTasks.has(row.dataset.taskTransitionId)) nameTaskRow(row);
                }
            }
            finish();
        };
        const observer = new MutationObserver(findPage);
        observer.observe(document.body, {
            childList: true, subtree: true, attributes: true,
            attributeFilter: ["data-recipes-index", "data-recipe-detail-id", "data-recipe-edit-id"],
        });
        const timeout = setTimeout(() => {
            transition.skipTransition();
            finish();
        }, 450);
        dioxus.send(true);
    }));
    transition.ready.catch(() => {});
    await transition.finished;
} finally {
    root.classList.remove(activeClass, returnClass, shoppingClass);
    outgoingPage?.classList.remove(outgoingClass);
    for (const property of properties) root.style.removeProperty(property);
    for (const row of namedRows) row.style.removeProperty("view-transition-name");
}
