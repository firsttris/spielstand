import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import {
  ActivityIcon,
  Gamepad2Icon,
  LayoutDashboardIcon,
  MonitorIcon,
  SettingsIcon,
  ShieldCheckIcon,
} from "lucide-react";
import { Dot } from "@/components/ui/misc";
import { api } from "@/lib/api";
import { cn } from "@/lib/utils";
import { m } from "@/paraglide/messages";

const linkClass =
  "flex h-9 items-center gap-2 rounded-md px-2 text-sm text-foreground/80 hover:bg-accent [&.active]:bg-accent [&.active]:font-medium [&.active]:text-foreground";

export function Sidebar() {
  const overview = useQuery({ queryKey: ["overview"], queryFn: api.overview, refetchInterval: 10_000 });
  const o = overview.data;
  const mirrorOk = o && (!o.mirror.enabled || (!o.mirror.lastError && o.mirror.pending === 0));

  return (
    <nav
      aria-label={m.nav_label()}
      className="flex w-full flex-col gap-0.5 border-b bg-sidebar p-2 sm:min-h-screen sm:w-auto sm:max-w-64 sm:min-w-52 sm:flex-[1_1_224px] sm:border-r sm:border-b-0"
    >
      <div className="flex items-center gap-2.5 px-2 pt-2 pb-4">
        <span className="flex size-8 items-center justify-center rounded-lg bg-primary text-primary-foreground">
          <ShieldCheckIcon className="size-[18px]" />
        </span>
        <span className="flex flex-col leading-tight">
          <span className="font-semibold">Spielstand</span>
          <span className="text-xs text-muted-foreground">{m.nav_server()}</span>
        </span>
      </div>
      <span className="px-2 pt-2 pb-1 text-xs font-medium text-muted-foreground">{m.nav_section()}</span>
      <Link to="/" className={linkClass} activeOptions={{ exact: true }}>
        <LayoutDashboardIcon className="size-4" /> {m.nav_overview()}
      </Link>
      <Link to="/games" className={linkClass}>
        <Gamepad2Icon className="size-4" /> {m.nav_games()}
        {o && <span className="ml-auto font-mono text-xs text-muted-foreground">{o.games}</span>}
      </Link>
      <Link to="/devices" className={linkClass}>
        <MonitorIcon className="size-4" /> {m.nav_devices()}
        {o && <span className="ml-auto font-mono text-xs text-muted-foreground">{o.devices}</span>}
      </Link>
      <Link to="/activity" className={linkClass}>
        <ActivityIcon className="size-4" /> {m.nav_activity()}
      </Link>
      <Link to="/settings" className={linkClass}>
        <SettingsIcon className="size-4" /> {m.nav_settings()}
      </Link>
      {o && (
        <div className="mt-4 flex flex-col gap-1 rounded-lg border bg-background p-3 sm:mt-auto">
          <span className="flex items-center gap-2 font-medium">
            <Dot className={cn(mirrorOk ? "bg-emerald-400" : "bg-amber-400")} /> {m.nav_server_status()}
          </span>
          <span className="text-xs text-muted-foreground">
            {m.nav_agents_online({ online: o.devicesOnline, total: o.devices })}
            {o.mirror.enabled &&
              ` · ${o.mirror.pending === 0 ? m.mirror_current() : m.mirror_pending({ n: o.mirror.pending })}`}
          </span>
        </div>
      )}
    </nav>
  );
}
