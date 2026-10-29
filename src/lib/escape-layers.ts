type EscapeLayer = {
  handle: (event: KeyboardEvent) => void;
  active: () => boolean;
};

const layers: EscapeLayer[] = [];

function captureEscape(event: KeyboardEvent) {
  if (
    event.key !== "Escape" ||
    event.defaultPrevented ||
    event.isComposing ||
    event.keyCode === 229
  )
    return;
  // A layer may leave an event inside its own DOM to the focused control.
  // It still owns that event: an older background popup must not receive it.
  for (let index = layers.length - 1; index >= 0; index--) {
    const layer = layers[index]!;
    if (layer.active()) {
      layer.handle(event);
      return;
    }
  }
}

export function registerEscapeLayer(
  handle: (event: KeyboardEvent) => void,
  active: () => boolean = () => true,
): () => void {
  if (!layers.length) document.addEventListener("keydown", captureEscape, true);
  const layer = { handle, active };
  layers.push(layer);
  return () => {
    const index = layers.indexOf(layer);
    if (index >= 0) layers.splice(index, 1);
    if (!layers.length)
      document.removeEventListener("keydown", captureEscape, true);
  };
}
