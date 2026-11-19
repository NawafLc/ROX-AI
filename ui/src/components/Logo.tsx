/** ROX-AI mark: a rounded tile with a stylised R-block and a spark. */
export function Logo({ size = 32 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" className="shrink-0">
      <defs>
        <linearGradient id="lg" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#19e8b9" />
          <stop offset="1" stopColor="#0094c6" />
        </linearGradient>
      </defs>
      <rect width="64" height="64" rx="16" fill="url(#lg)" />
      <path d="M20 46V18h14.5c6 0 10 3.6 10 9 0 4-2.2 6.9-5.8 8.2L45 46h-7.6l-5.6-10H27v10h-7Zm7-16h7c2.4 0 3.9-1.3 3.9-3.3S36.4 23.4 34 23.4h-7V30Z" fill="#04140f" />
      <circle cx="47.5" cy="16.5" r="3.5" fill="#04140f" />
    </svg>
  )
}
