import { RotateCcw, TriangleAlert } from "lucide-react";
import { BrandMark } from "./BrandMark";

type ErrorScreenProps = {
  message: string;
  onRetry: () => void;
};

export function ErrorScreen({ message, onRetry }: ErrorScreenProps) {
  return (
    <main className="system-screen system-screen--error">
      <BrandMark />
      <span className="system-screen__icon"><TriangleAlert size={24} /></span>
      <h1>CrowClaw could not open</h1>
      <p>{message}</p>
      <button className="button button--primary" type="button" onClick={onRetry}><RotateCcw size={17} /> Try again</button>
    </main>
  );
}

