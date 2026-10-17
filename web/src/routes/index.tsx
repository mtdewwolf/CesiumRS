import { createFileRoute } from "@tanstack/react-router";
import { GlobeApp } from "@/components/globe-app";

export const Route = createFileRoute("/")({ component: Home });

function Home() {
  return <GlobeApp />;
}
