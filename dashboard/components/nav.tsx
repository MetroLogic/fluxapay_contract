"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useState } from "react";
import { isAuthenticated, signOut } from "@/lib/auth";

const LINKS = [
  { href: "/payments", label: "Payments", icon: "💳" },
  { href: "/disputes", label: "Disputes", icon: "⚖️" },
  { href: "/refunds", label: "Refunds", icon: "↩️" },
  { href: "/links", label: "Links", icon: "🔗" },
  { href: "/settlements", label: "Settlements", icon: "🏦" },
  { href: "/settings", label: "Settings", icon: "⚙️" },
];

/**
 * Issue #1165: shared dashboard navigation shell (responsive sidebar +
 * topbar) with active-route highlighting.
 */
export function Nav() {
  const pathname = usePathname();
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const isActive = (href: string) => pathname === href || pathname.startsWith(`${href}/`);

  return (
    <div className="min-h-screen lg:flex">
      <aside
        className={`${
          open ? "block" : "hidden"
        } w-full border-b border-slate-200 bg-white p-4 dark:border-slate-800 dark:bg-slate-900 lg:block lg:w-60 lg:border-b-0 lg:border-r`}
      >
        <div className="mb-6 flex items-center justify-between">
          <Link href="/payments" className="text-lg font-semibold">FluxaPay</Link>
          <button
            type="button"
            aria-label="Toggle navigation"
            onClick={() => setOpen((v) => !v)}
            className="lg:hidden"
          >
            ☰
          </button>
        </div>
        <nav aria-label="Dashboard" className="space-y-1">
          {LINKS.map((link) => (
            <Link
              key={link.href}
              href={link.href}
              aria-current={isActive(link.href) ? "page" : undefined}
              className={`flex items-center gap-2 rounded px-3 py-2 text-sm ${
                isActive(link.href)
                  ? "bg-sky-100 font-semibold text-sky-800 dark:bg-sky-900 dark:text-sky-100"
                  : "hover:bg-slate-100 dark:hover:bg-slate-800"
              }`}
              onClick={() => setOpen(false)}
            >
              <span aria-hidden="true">{link.icon}</span>
              {link.label}
            </Link>
          ))}
        </nav>
      </aside>
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center justify-between border-b border-slate-200 bg-white px-4 py-3 dark:border-slate-800 dark:bg-slate-900">
          <button
            type="button"
            aria-label="Toggle navigation"
            onClick={() => setOpen((v) => !v)}
            className="lg:hidden"
          >
            ☰
          </button>
          <span className="text-sm text-slate-500">Merchant Dashboard</span>
          {isAuthenticated() ? (
            <button
              type="button"
              className="text-sm text-sky-700 dark:text-sky-300"
              onClick={() => {
                signOut();
                router.replace("/login");
              }}
            >
              Sign out
            </button>
          ) : (
            <Link href="/login" className="text-sm text-sky-700 dark:text-sky-300">Sign in</Link>
          )}
        </header>
        <main className="mx-auto w-full max-w-6xl px-4 py-6 sm:px-6 lg:px-8">
          {children}
        </main>
      </div>
    </div>
  );
}
