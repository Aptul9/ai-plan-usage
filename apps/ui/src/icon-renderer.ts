const canvas = document.getElementById('c') as HTMLCanvasElement
const ctx = canvas.getContext('2d')!

;(window as any).renderIcon = function (style: IconStyle, payload: IconPayload): string {
  const W = 64
  const H = 64
  ctx.clearRect(0, 0, W, H)
  ctx.imageSmoothingEnabled = true
  ctx.imageSmoothingQuality = 'high'

  const cx = W / 2
  const cy = H / 2

  const sess = payload.session
  const week = payload.weekly
  const err = payload.error

  if (err) {
    ctx.fillStyle = '#666666'
    ctx.beginPath()
    ctx.arc(cx, cy, 28, 0, Math.PI * 2)
    ctx.fill()
    ctx.strokeStyle = 'white'
    ctx.lineWidth = 4
    ctx.lineCap = 'round'
    ctx.beginPath(); ctx.moveTo(20, 20); ctx.lineTo(44, 44); ctx.stroke()
    ctx.beginPath(); ctx.moveTo(44, 20); ctx.lineTo(20, 44); ctx.stroke()
    return canvas.toDataURL('image/png')
  }

  const sPct = sess.pct ?? 0
  const wPct = week.pct ?? 0
  const sCol = sess.color
  const wCol = week.color

  if (style === 'solid') {
    const pct = Math.max(sPct, wPct)
    const col = pct >= 95 ? '#dc2626' : pct >= 80 ? '#d97706' : '#22a06b'
    ctx.fillStyle = col
    ctx.beginPath(); ctx.arc(cx, cy, 28, 0, Math.PI * 2); ctx.fill()
  } else if (style === 'number') {
    ctx.fillStyle = sCol
    ctx.beginPath(); ctx.arc(cx, cy, 30, 0, Math.PI * 2); ctx.fill()
    ctx.fillStyle = 'white'
    ctx.font = 'bold 34px "Segoe UI", system-ui, sans-serif'
    ctx.textAlign = 'center'
    ctx.textBaseline = 'middle'
    ctx.fillText(String(Math.round(sPct)), cx, cy + 2)
  } else if (style === 'ring') {
    ctx.lineWidth = 9
    ctx.strokeStyle = '#d4d4d4'
    ctx.beginPath(); ctx.arc(cx, cy, 24, 0, Math.PI * 2); ctx.stroke()
    ctx.strokeStyle = wCol
    ctx.lineCap = 'round'
    ctx.beginPath()
    ctx.arc(cx, cy, 24, -Math.PI / 2, -Math.PI / 2 + (wPct / 100) * Math.PI * 2)
    ctx.stroke()
  } else if (style === 'ring+number') {
    ctx.lineWidth = 4
    ctx.strokeStyle = '#d4d4d4'
    ctx.beginPath(); ctx.arc(cx, cy, 30, 0, Math.PI * 2); ctx.stroke()
    ctx.strokeStyle = wCol
    ctx.lineCap = 'round'
    ctx.beginPath()
    ctx.arc(cx, cy, 30, -Math.PI / 2, -Math.PI / 2 + (wPct / 100) * Math.PI * 2)
    ctx.stroke()
    ctx.fillStyle = sCol
    ctx.font = 'bold 36px "Segoe UI", system-ui, sans-serif'
    ctx.textAlign = 'center'
    ctx.textBaseline = 'middle'
    ctx.fillText(String(Math.round(sPct)), cx, cy + 2)
  } else if (style === 'bar') {
    const padX = 12
    const padY = 6
    const barW = W - padX * 2
    const barH = H - padY * 2
    const fillH = Math.round((sPct / 100) * barH)
    ctx.fillStyle = '#e5e5e5'
    ctx.fillRect(padX, padY, barW, barH)
    ctx.fillStyle = sCol
    ctx.fillRect(padX, padY + (barH - fillH), barW, fillH)
    ctx.lineWidth = 2
    ctx.strokeStyle = '#333'
    ctx.strokeRect(padX, padY, barW, barH)
  }

  return canvas.toDataURL('image/png')
}
