import Link from "next/link";

/** Issue #1165: shared heading for dashboard sections. */
export function PlaceholderPage({ title, description }: { title: string; description: string }) {
  return (
    <section>
      <h1 className="mb-2 text-2xl font-semibold">{title}</h1>
      <p className="text-slate-500">{description}</p>
      <p className="mt-4 text-sm">
        See <Link className="text-sky-700 underline dark:text-sky-300" href="/payments">payments</Link> for the full management view.
      </p>
    </section>
  );
}
