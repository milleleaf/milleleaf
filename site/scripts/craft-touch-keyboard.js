// Lets phones and tablets open their on-screen keyboard in this app. The app types through a hidden
// text field, but it focuses that field one frame after the tap that starts editing, and mobile
// browsers open the keyboard only for focus given during a tap. So on a tap (short, barely moving)
// on the app, focus the field during the tap: where the app wants text it keeps the field focused
// and the keyboard stays; elsewhere it moves focus back to its canvas on the next frame.
(() => {
  let start = null
  addEventListener('touchstart', (event) => {
    const touch = event.touches[0]
    start = event.touches.length === 1 && event.target instanceof HTMLCanvasElement ? { x: touch.clientX, y: touch.clientY, time: Date.now() } : null
  }, { capture: true, passive: true })
  addEventListener('touchend', (event) => {
    const touch = event.changedTouches[0]
    const tap = start && touch && Date.now() - start.time < 600 && Math.hypot(touch.clientX - start.x, touch.clientY - start.y) < 12
    start = null
    const field = document.querySelector('input[type=text]')
    if (tap && field && document.activeElement !== field) field.focus({ preventScroll: true })
  }, { capture: true, passive: true })
})()
