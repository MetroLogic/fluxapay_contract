"use client";

/**
 * Issue #1158: layout.tsx imports { Nav } from "@/components/nav".
 * The component did not exist, contributing to the build failure.
 *
 * Minimal navigation bar shown on every dashboard page. When the merchant
 * is authenticated (JWT in localStorage) it shows their wallet address and a
 * sign-out button; otherwise it shows a link to the home page to connect.
 */

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";

const NAV_ITEMS: Array<{ href: string; label: string }> = [
  { href: "/invoices", label: "Invoices" },
  { href: "/payment-links", label: "Payment links" },
  { href: "/subscriptions", label: "Subscriptions" },
  { href: "/settings/webhooks", label: "Webhooks" },
];

export function Nav() {
  const pathname = usePathname();
  const [jwt, setJwt] = useState<string | null>(null);
  const [walletAddress, setWalletAddress] = useState<string | null>(null);

  useEffect(() => {
    if (typeof window === "undefined") return;
    setJwt(localStorage.getItem("fluxapay_jwt"));
    setWalletAddress(localStorage.getItem("fluxapay_wallet_address"));
  }, []);

  const handleSignOut = () => {
    localStorage.removeItem("fluxapay_jwt");
    localStorage.removeItem("fluxapay_wallet_address");
    window.location.href = "/";
  };

  const isActive = (href: string) =>
    pathname === href || (href !== "/" && pathname.startsWith(href));

  return (
    <nav
      className="border-b border-slate-200 bg-white dark:border-slate-700 dark:bg-slate-900"
      aria-label="Main navigation"
    >
      <div className="mx-auto flex max-w-6xl items-center justify-between px-4 py-3 sm:px-6 lg:px-8">
        <Link
          href="/"
          className="text-sm font-semibold text-slate-900 dark:text-slate-100"
        >
          FluxaPay
        </Link>

        {jwt && (
          <ul className="flex items-center gap-1">
            {NAV_ITEMS.map((item) => (
              <li key={item.href}>
                <Link
                  href={item.href}
                  className={`rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${
                    isActive(item.href)
                      ? "bg-indigo-50 text-indigo-700 dark:bg-indigo-900/40 dark:text-indigo-300"
                      : "text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800"
                  }`}
                >
                  {item.label}
                </Link>
              </li>
            ))}
          </ul>
        )}

        <div className="flex items-center gap-3">
          {jwt && walletAddress ? (
            <>
              <span className="hidden font-mono text-xs text-slate-500 dark:text-slate-400 sm:inline">
                {walletAddress.slice(0, 4)}…{walletAddress.slice(-4)}
              </span>
              <button
                type="button"
                onClick={handleSignOut}
                className="rounded-md border border-slate-300 px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-100 dark:border-slate-600 dark:text-slate-200 dark:hover:bg-slate-800"
              >
                Sign out
              </button>
            </>
          ) : (
            <Link
              href="/"
              className="rounded-md bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-700"
            >
              Connect wallet
            </Link>
          )}
        </div>
      </div>
    </nav>
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
