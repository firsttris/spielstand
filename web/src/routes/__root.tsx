import { type QueryClient, useQuery } from "@tanstack/react-query";
import { createRootRouteWithContext, Navigate, Outlet, useLocation } from "@tanstack/react-router";
import { Sidebar } from "@/components/sidebar";
import { api } from "@/lib/api";

export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()({
  component: Root,
});

function Root() {
  const location = useLocation();
  const session = useQuery({ queryKey: ["session"], queryFn: api.session });
  const onLogin = location.pathname === "/login";

  if (session.isPending) return null;
  const loggedIn = session.data?.loggedIn ?? false;
  if (!loggedIn && !onLogin) return <Navigate to="/login" />;
  if (loggedIn && onLogin) return <Navigate to="/" />;
  if (onLogin) return <Outlet />;

  return (
    <div className="flex min-h-screen flex-wrap">
      <Sidebar />
      <main className="min-w-0 flex-[999_1_560px] px-4 py-6 sm:px-8">
        <div className="mx-auto flex w-full max-w-[1600px] flex-col gap-6">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
