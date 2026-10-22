import { useMutation, useQuery } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Dot } from "@/components/ui/misc";
import { api } from "@/lib/api";
import { m } from "@/paraglide/messages";

/** Shows the one-line install command and waits until the new device shows up. */
export function PairDialog({ children }: { children: React.ReactNode }) {
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const known = useRef<Set<string>>(new Set());
  const code = useMutation({ mutationFn: api.pairCode });
  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: api.devices,
    refetchInterval: open ? 2_000 : 10_000,
  });
  const fresh = (devices.data ?? []).find((d) => !known.current.has(d.id));
  const { mutate } = code;

  // Only when the dialog opens: snapshot the devices known before pairing.
  // biome-ignore lint/correctness/useExhaustiveDependencies: devices are read once on open
  useEffect(() => {
    if (open) {
      known.current = new Set((devices.data ?? []).map((d) => d.id));
      mutate();
    }
  }, [open, mutate]);

  const copy = async () => {
    if (!code.data) return;
    await navigator.clipboard?.writeText(code.data.command);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent className="max-w-2xl">
        <DialogTitle>{m.pair_title()}</DialogTitle>
        <DialogDescription>{m.pair_text()}</DialogDescription>
        <div className="flex flex-wrap items-center gap-2">
          <code className="min-w-0 flex-[1_1_420px] overflow-x-auto rounded-md border bg-background px-3 py-2.5 font-mono text-[13px] whitespace-nowrap">
            {code.data?.command ?? "…"}
          </code>
          <Button variant="outline" onClick={copy} disabled={!code.data}>
            {copied ? m.copied() : m.copy()}
          </Button>
        </div>
        <ol className="flex list-decimal flex-col gap-1 pl-5 text-[13px] text-muted-foreground">
          <li>{m.pair_step1()}</li>
          <li>{m.pair_step2()}</li>
          <li>{m.pair_step3()}</li>
        </ol>
        <p className="text-[13px] text-muted-foreground">
          {m.pair_update()}{" "}
          <code className="font-mono text-foreground">{code.data?.updateCommand ?? "…"}</code>
        </p>
        {fresh ? (
          <p className="flex items-center gap-2 text-sm text-success">
            <Dot className="bg-emerald-400" /> {m.pair_done({ device: fresh.name })}
          </p>
        ) : (
          <p className="flex items-center gap-2 text-[13px] text-muted-foreground">
            <Dot className="animate-pulse bg-blue-400" /> {m.pair_waiting()}
          </p>
        )}
      </DialogContent>
    </Dialog>
  );
}
