export default function HomePage() {
  return (
    <main
      style={{
        fontFamily: "system-ui, sans-serif",
        padding: "2rem",
        maxWidth: "60rem",
        lineHeight: 1.6,
      }}
    >
      <h1>Replayable</h1>
      <p>
        Framework- and language-agnostic agent trace capture, replay, and evaluation.
      </p>
      <p>
        Status: v0.0.1 pre-alpha. See <code>docs/ARCHITECTURE.md</code> for the
        plan.
      </p>
    </main>
  );
}
