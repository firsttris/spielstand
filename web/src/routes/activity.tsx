import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { ActivityList } from "@/components/activity-list";
import { Card, CardContent } from "@/components/ui/card";
import { PageHeader, Skeleton } from "@/components/ui/misc";
import { api } from "@/lib/api";
import { dateTime } from "@/lib/format";
import { m } from "@/paraglide/messages";

export const Route = createFileRoute("/activity")({ component: ActivityPage });

function ActivityPage() {
  const activity = useQuery({
    queryKey: ["activity", 200],
    queryFn: () => api.activity(200),
    refetchInterval: 5_000,
  });
  return (
    <>
      <PageHeader title={m.nav_activity()} description={m.activity_subtitle()} />
      <Card>
        <CardContent>
          {!activity.data ? (
            <Skeleton className="h-64" />
          ) : activity.data.length === 0 ? (
            <p className="text-center text-sm text-muted-foreground">{m.activity_empty()}</p>
          ) : (
            <ActivityList items={activity.data} when={dateTime} />
          )}
        </CardContent>
      </Card>
    </>
  );
}
