import type * as React from "react";
import { cn, initials, tileColor } from "@/lib/utils";

export function Separator({ className }: { className?: string }) {
  return <hr className={cn("h-px border-0 bg-border", className)} />;
}

export function Skeleton({ className }: { className?: string }) {
  return <div className={cn("animate-pulse rounded-md bg-muted", className)} />;
}

/** The coloured square with initials that stands in for game art. */
export function GameTile({ title, size = 36 }: { title: string; size?: number }) {
  return (
    <span
      aria-hidden="true"
      className="flex shrink-0 items-center justify-center rounded-lg font-semibold text-zinc-950"
      style={{ width: size, height: size, background: tileColor(title), fontSize: Math.round(size / 3) }}
    >
      {initials(title)}
    </span>
  );
}

export function Dot({ className }: { className?: string }) {
  return <span aria-hidden="true" className={cn("inline-block size-2 shrink-0 rounded-full", className)} />;
}

/** Pages of forms and lists stay readable on wide screens; card grids use the full width. */
export function NarrowPage({ children }: { children: React.ReactNode }) {
  return <div className="flex max-w-[1040px] flex-col gap-6">{children}</div>;
}

export function PageHeader({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children?: React.ReactNode;
}) {
  return (
    <header className="flex flex-wrap items-center gap-3">
      <div className="flex flex-col gap-0.5">
        <h1 className="text-2xl font-semibold tracking-tight">{title}</h1>
        {description && <p className="text-muted-foreground">{description}</p>}
      </div>
      {children && <div className="ml-auto flex flex-wrap gap-2">{children}</div>}
    </header>
  );
}
