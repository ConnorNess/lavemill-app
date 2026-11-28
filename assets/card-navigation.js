const [page, returningHome, recipeId] = await dioxus.recv();
const root = document.documentElement;
const activeClass = "home-card-transition";
const returnClass = "home-card-return";
const deckClass = "home-card-deck";

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
const namedElements = [];
const nameElement = (element, name) => {
    element.style.viewTransitionName = name;
    namedElements.push(element);
};
const nameDeck = (selected) => {
    const cards = Array.from(selected.parentElement.querySelectorAll(":scope > .dashboard-card"));
    const selectedIndex = cards.indexOf(selected);
    for (const [index, card] of cards.entries()) {
        const offset = index - selectedIndex;
        if (offset === 0) {
            nameElement(card, "navigation-panel");
        } else {
            const before = offset < 0;
            const direction = before ? "before" : "after";
            const delayed = before ? index > 0 : offset === 1;
            nameElement(card, `dashboard-card-${direction}${delayed ? "-delayed" : ""}`);
        }
    }
};
const outgoingPage = openingFromHeader || recipeKey !== null ? interiorPage : null;
const outgoingClass = "home-card-source";
const deckTransition = ["shopping", "todos", "calendar"].includes(page);

root.classList.add(activeClass);
root.classList.toggle(returnClass, returningHome);
root.classList.toggle(deckClass, deckTransition);
try {
    outgoingPage?.classList.add(outgoingClass);
    if (deckTransition) {
        if (returningHome) nameElement(source, "navigation-panel");
        else nameDeck(source);
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
            if (deckTransition) {
                if (returningHome) {
                    const landingCard = findCard();
                    if (!landingCard) return;
                    landingCard.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" });
                    nameDeck(landingCard);
                } else {
                    nameElement(destination, "navigation-panel");
                }
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
    root.classList.remove(activeClass, returnClass, deckClass);
    outgoingPage?.classList.remove(outgoingClass);
    for (const property of properties) root.style.removeProperty(property);
    for (const element of namedElements) element.style.removeProperty("view-transition-name");
}
