import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { ShieldCheckIcon } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input, Label } from "@/components/ui/input";
import { ApiError, api } from "@/lib/api";
import { m } from "@/paraglide/messages";

export const Route = createFileRoute("/login")({ component: Login });

function Login() {
  const client = useQueryClient();
  const session = useQuery({ queryKey: ["session"], queryFn: api.session });
  const setup = session.data?.setupRequired ?? false;
  const [password, setPassword] = useState("");
  const submit = useMutation({
    mutationFn: () => (setup ? api.setup(password) : api.login(password)),
    onSuccess: (info) => client.setQueryData(["session"], info),
  });
  const error =
    submit.error instanceof ApiError && submit.error.status === 401 ? m.login_wrong() : submit.error?.message;

  return (
    <main className="flex min-h-screen items-center justify-center p-4">
      <Card className="flex w-full max-w-sm flex-col gap-5 p-6">
        <div className="flex items-center gap-3">
          <span className="flex size-10 items-center justify-center rounded-lg bg-primary text-primary-foreground">
            <ShieldCheckIcon className="size-5" />
          </span>
          <div>
            <h1 className="text-lg font-semibold">{setup ? m.setup_title() : m.login_title()}</h1>
            <p className="text-sm text-muted-foreground">{setup ? m.setup_text() : m.login_text()}</p>
          </div>
        </div>
        <form
          className="flex flex-col gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            submit.mutate();
          }}
        >
          <Label htmlFor="password">{m.login_password()}</Label>
          <Input
            id="password"
            type="password"
            autoComplete={setup ? "new-password" : "current-password"}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            minLength={setup ? 8 : undefined}
            required
            autoFocus
          />
          {error && (
            <p role="alert" className="text-sm text-destructive">
              {error}
            </p>
          )}
          <Button type="submit" disabled={submit.isPending}>
            {setup ? m.setup_submit() : m.login_submit()}
          </Button>
        </form>
      </Card>
    </main>
  );
}
