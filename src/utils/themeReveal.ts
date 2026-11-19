import { nextTick } from "vue";

let activeTransition: ViewTransition | undefined;

export function revealTheme(button: HTMLElement, applyTheme: () => void) {
  activeTransition?.skipTransition();
  if (!document.startViewTransition || window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    applyTheme();
    return;
  }

  const bounds = button.getBoundingClientRect();
  const x = bounds.left + bounds.width / 2;
  const y = bounds.top + bounds.height / 2;
  const radius = Math.ceil(Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y)));
  const root = document.documentElement;
  root.classList.add("theme-reveal");
  const transition = document.startViewTransition(async () => {
    applyTheme();
    await nextTick();
  });
  activeTransition = transition;

  void transition.ready.then(() => {
    root.animate({
      clipPath: ["circle(0px at " + x + "px " + y + "px)", "circle(" + radius + "px at " + x + "px " + y + "px)"],
    }, {
      duration: 460,
      easing: "ease-out",
      pseudoElement: "::view-transition-new(root)",
    });
  }).catch(() => { /* A skipped animation still applies the theme. */ });

  const finish = () => {
    if (activeTransition !== transition) return;
    activeTransition = undefined;
    root.classList.remove("theme-reveal");
  };
  void transition.finished.then(finish, finish);
}
