// Applies only to the HTML settings UI. Native chart preview/play views keep
// their own multitouch input. Some WKWebView versions allow page pinch zoom
// despite viewport limits, so also cancel their gesture events explicitly.
export function installIosPageZoomGuard() {
  const viewport = document.querySelector<HTMLMetaElement>('meta[name="viewport"]');
  if (viewport) viewport.content += ', maximum-scale=1.0, user-scalable=no';

  const preventGesture = (event: Event) => event.preventDefault();
  const preventPinch = (event: TouchEvent) => {
    if (event.touches.length > 1) event.preventDefault();
  };
  const options = { passive: false, capture: true };
  for (const name of ['gesturestart', 'gesturechange', 'gestureend']) {
    document.addEventListener(name, preventGesture, options);
  }
  document.addEventListener('touchmove', preventPinch, options);
}
