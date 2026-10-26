// Lets phones and tablets open their on-screen keyboard in this app. The app types through a hidden
// text field, but it focuses that field a frame or two after the tap that starts editing, and mobile
// browsers open the keyboard only for focus given during a tap. On each frame before it starts
// editing, the app also hands focus back to its canvas, which closes a keyboard opened by the tap.
//
// So on the second tap of a double-tap (how a cell or text box starts editing) on the app, focus the
// field during the tap, then hold it for a few frames: the app's requests to blur the field or focus
// its canvas wait until it decides. If it starts editing (it moves the field to the text cursor),
// the field keeps focus and the keyboard stays up; if not, the waiting requests are carried out.
// Single taps leave the field alone, so they never flash the keyboard; while the app is already
// editing, it re-focuses the field on each tap itself.
(() => {
  const HOLD_FRAMES = 8
  let start = null
  let lastTap = null
  let hold = null

  const field = () => document.querySelector('input[type=text]')

  // Defers the app's blur of the field and focus of its canvas while a hold is on.
  const patched = new WeakSet()
  const patch = (input, canvas) => {
    if (patched.has(input)) return
    patched.add(input)
    const blur = input.blur.bind(input)
    input.blur = () => { if (hold) hold.blur = true; else blur() }
    const focusCanvas = canvas.focus.bind(canvas)
    canvas.focus = (options) => { if (hold) hold.canvas = options || {}; else focusCanvas(options) }
    // The app moves the field to the text cursor when it starts editing.
    new MutationObserver(() => { if (hold) hold.editing = true }).observe(input, { attributes: true, attributeFilter: ['style'] })
    patch.release = () => {
      const pending = hold
      hold = null
      if (pending && !pending.editing && (pending.blur || pending.canvas)) {
        if (pending.blur) blur()
        if (pending.canvas) focusCanvas(pending.canvas)
      }
    }
  }

  addEventListener('touchstart', (event) => {
    const touch = event.touches[0]
    start = event.touches.length === 1 && event.target instanceof HTMLCanvasElement ? { x: touch.clientX, y: touch.clientY, time: Date.now() } : null
  }, { capture: true, passive: true })

  addEventListener('touchend', (event) => {
    const touch = event.changedTouches[0]
    const now = Date.now()
    const tap = start && touch && now - start.time < 600 && Math.hypot(touch.clientX - start.x, touch.clientY - start.y) < 12
    start = null
    if (!tap) { lastTap = null; return }
    const second = lastTap && now - lastTap.time < 450 && Math.hypot(touch.clientX - lastTap.x, touch.clientY - lastTap.y) < 30
    lastTap = second ? null : { x: touch.clientX, y: touch.clientY, time: now }
    const input = field()
    if (!second || !input || !(event.target instanceof HTMLCanvasElement)) return
    patch(input, event.target)
    hold = { blur: false, canvas: null, editing: false }
    if (document.activeElement !== input) input.focus({ preventScroll: true })
    let frames = 0
    const tick = () => { if (!hold) return; if (hold.editing || ++frames >= HOLD_FRAMES) patch.release(); else requestAnimationFrame(tick) }
    requestAnimationFrame(tick)
  }, { capture: true, passive: true })
})()
