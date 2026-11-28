(() => {
    const root = document.documentElement;
    const activeClass = "home-card-transition";
    const returnClass = "home-card-return";
    const deckClass = "home-card-deck";
    const pageSelector = "main#main[data-navigation-page]";
    const headerSelector = ".site-header";
    const panelName = "navigation-panel";
    const sceneName = "navigation-scene";
    const anchorSelector = (path) => `[data-navigation-anchor="${CSS.escape(path)}"]`;
    const findCard = (home, path) => home.querySelector(anchorSelector(path))?.closest(".dashboard-card");
    const findHeaderAnchor = (path) => document.querySelector(headerSelector)
        ?.querySelector(anchorSelector(path));
    const isVisible = (element) => {
        if (!element) return false;
        const { top, bottom, left, right } = element.getBoundingClientRect();
        return bottom > 0 && top < root.clientHeight && right > 0 && left < root.clientWidth;
    };

    window.lavemillNavigate = async (path, navigate, { replacing = false, blocked } = {}) => {
        if (root.classList.contains(activeClass)) {
            if (replacing) navigate();
            else blocked?.();
            return;
        }

        const outgoingPage = document.querySelector(pageSelector);
        const from = outgoingPage?.dataset.navigationPage;
        const to = new URL(path, location.href).pathname;
        if (!from || from === to || !document.startViewTransition ||
            matchMedia("(prefers-reduced-motion: reduce)").matches) {
            navigate();
            return;
        }

        const returning = to === "/" || outgoingPage.dataset.navigationParent === to;
        const launchCard = from === "/" ? findCard(outgoingPage, to) : null;
        const launchAnchor = returning ? null :
            outgoingPage.querySelector(anchorSelector(to)) ?? findHeaderAnchor(to);
        const outgoingPanel = launchCard ?? (isVisible(launchAnchor) ? launchAnchor : outgoingPage);
        const namedElements = new Map();
        const nameElement = (element, name) => {
            if (!namedElements.has(element)) namedElements.set(element, element.style.viewTransitionName);
            element.style.viewTransitionName = name;
        };
        const nameDeck = (selected) => {
            const cards = Array.from(selected.parentElement.querySelectorAll(":scope > .dashboard-card"));
            const selectedIndex = cards.indexOf(selected);
            for (const [index, card] of cards.entries()) {
                const offset = index - selectedIndex;
                if (offset === 0) continue;

                const before = offset < 0;
                const direction = before ? "before" : "after";
                const delayed = before ? index > 0 : offset === 1;
                nameElement(card, `dashboard-card-${direction}${delayed ? "-delayed" : ""}`);
            }
            root.classList.add(deckClass);
        };
        let navigated = false;
        const navigateOnce = () => {
            if (navigated) return;
            navigated = true;
            navigate();
        };

        root.classList.add(activeClass);
        root.classList.toggle(returnClass, returning);
        try {
            nameElement(outgoingPanel, panelName);
            if (outgoingPanel !== outgoingPage) nameElement(outgoingPage, sceneName);
            if (launchCard) nameDeck(launchCard);

            const transition = document.startViewTransition(() => new Promise((resolve) => {
                const finish = () => {
                    observer.disconnect();
                    clearTimeout(timeout);
                    resolve();
                };
                const findPage = () => {
                    const incomingPage = document.querySelector(
                        `${pageSelector}[data-navigation-page="${CSS.escape(to)}"]`,
                    );
                    if (!incomingPage) return;

                    const landingCard = to === "/" ? findCard(incomingPage, from) : null;
                    const section = from.split("/", 2).join("/");
                    const landingAnchor = to === "/" ? findHeaderAnchor(section) :
                        incomingPage.querySelector(anchorSelector(from));
                    const incomingPanel = returning ? landingCard ?? landingAnchor ?? incomingPage : incomingPage;
                    if (incomingPanel !== incomingPage) {
                        incomingPanel.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" });
                        nameElement(incomingPage, sceneName);
                    }
                    nameElement(incomingPanel, panelName);
                    if (landingCard) nameDeck(landingCard);
                    finish();
                };
                const observer = new MutationObserver(findPage);
                observer.observe(document.body, {
                    childList: true, subtree: true, attributes: true,
                    attributeFilter: ["data-navigation-page"],
                });
                const timeout = setTimeout(() => {
                    transition.skipTransition();
                    finish();
                }, 450);
                navigateOnce();
                findPage();
            }));
            transition.ready.catch(() => {});
            await transition.finished;
        } catch {
            navigateOnce();
        } finally {
            root.classList.remove(activeClass, returnClass, deckClass);
            for (const [element, name] of namedElements) element.style.viewTransitionName = name;
        }
    };
})();
