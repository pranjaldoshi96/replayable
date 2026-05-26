import type { ReactNode } from "react";

export const metadata = {
  title: "Replayable",
  description: "Open-source agent trace capture, replay, and evaluation",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
