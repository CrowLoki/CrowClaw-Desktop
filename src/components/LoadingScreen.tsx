import { LoaderCircle } from "lucide-react";
import { BrandMark } from "./BrandMark";

export function LoadingScreen() {
  return (
    <main className="system-screen" aria-busy="true">
      <BrandMark />
      <LoaderCircle className="spin" size={24} />
      <p>Opening your local workspace…</p>
    </main>
  );
}

