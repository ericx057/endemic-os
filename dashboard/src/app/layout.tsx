import type { Metadata } from 'next'
import type { ReactNode } from 'react'
import '@/index.css'

export const metadata: Metadata = { title: 'Endemic', icons: '/favicon.svg' }

const NavLink = ({ href, children }: { href: string; children: string }) => (
  <a href={href} className="text-sm text-white/70 underline-offset-4 hover:text-white hover:underline">
    {children}
  </a>
)

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body>
        <div className="mx-auto flex min-h-screen max-w-[1600px] flex-col px-4 sm:px-6">
          <nav aria-label="Primary" className="flex items-center justify-between py-4">
            <a href="/" className="display text-xl font-bold tracking-tight">
              Endemic
            </a>
            <div className="flex items-center gap-5">
              <NavLink href="/">Dashboard</NavLink>
              <NavLink href="/setup">Setup</NavLink>
            </div>
          </nav>
          <main className="flex-1">{children}</main>
        </div>
      </body>
    </html>
  )
}
