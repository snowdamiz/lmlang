import { useMemo, useState } from "react";
import {
  ArrowRight,
  Binary,
  Bot,
  BrainCircuit,
  CheckCircle2,
  Cpu,
  GitBranch,
  Layers3,
  Radar,
  Sparkles,
  TerminalSquare,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button, buttonVariants } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

type ConsoleState = "idle" | "loading" | "success" | "error" | "empty";

const valuePillars = [
  {
    title: "Program-as-data execution, not text patching",
    description:
      "lmlang treats programs as structured graph state first, so agents act on real runtime structure instead of brittle source-level guesses.",
    icon: Layers3,
  },
  {
    title: "Versioned planner contract for predictable runs",
    description:
      "Goals are translated into validated planner actions with a dated contract, giving teams deterministic behavior they can audit after every run.",
    icon: Cpu,
  },
  {
    title: "One chat loop from setup to verified build",
    description:
      "Teams can create projects, configure agents, and launch build workflows from one dashboard chat surface without context switching.",
    icon: Bot,
  },
];

const workflowSteps = [
  {
    title: "Open dashboard and configure your agent stack",
    description:
      "Set provider, model, and API key once to establish a repeatable operator control loop.",
  },
  {
    title: "Create a project and assign the build agent",
    description:
      "Use chat prompts to create scope, register an agent, and bind execution to the selected workspace.",
  },
  {
    title: "Start a goal-driven autonomous run",
    description:
      "Your build goal routes through the planner contract and executes as ordered actions with runtime verify checks.",
  },
  {
    title: "Inspect state, retry safely, and recover fast",
    description:
      "History, diagnostics, checkpoints, and graph diff tooling make failures easier to triage and safer to roll back.",
  },
];

const trustRows = [
  {
    title: "Locking discipline for concurrent agent edits",
    description:
      "Function-level locks with auto-expiry and conflict detection reduce collisions when multiple agents mutate the same runtime.",
    icon: Radar,
  },
  {
    title: "Post-execution verify gates with bounded retries",
    description:
      "Runs execute deterministic verification and retry before surfacing terminal failures, reducing false confidence during delivery.",
    icon: GitBranch,
  },
  {
    title: "Persistent history with checkpoints and undo/redo",
    description:
      "Teams can diff runtime state, restore checkpoints, and reverse mutations without rebuilding context from scratch.",
    icon: CheckCircle2,
  },
  {
    title: "Incremental compile visibility plus contract tests",
    description:
      "Dirty-status compile feedback and contract property testing keep iteration tight while preserving correctness guarantees.",
    icon: Binary,
  },
];

const commandSuggestions = [
  "create project calculator-demo",
  "assign agent",
  "start build calculator workflow with verify and compile",
];

const toneClassByState: Record<ConsoleState, string> = {
  idle: "border-border bg-background/90 text-foreground",
  loading: "border-primary/35 bg-primary/10 text-primary",
  success: "border-emerald-500/35 bg-emerald-100/75 text-emerald-900",
  error: "border-rose-500/35 bg-rose-100/75 text-rose-900",
  empty: "border-amber-500/35 bg-amber-100/75 text-amber-900",
};

function App() {
  const [command, setCommand] = useState(commandSuggestions[0]);
  const [consoleState, setConsoleState] = useState<ConsoleState>("idle");
  const [output, setOutput] = useState(
    "Planner idle. Queue a dashboard chat goal to simulate the operator loop.",
  );
  const [lastRunAt, setLastRunAt] = useState<string>("");

  const statusLabel = useMemo(() => {
    switch (consoleState) {
      case "loading":
        return "Planning";
      case "success":
        return "Accepted";
      case "error":
        return "Rejected";
      case "empty":
        return "Missing input";
      default:
        return "Ready";
    }
  }, [consoleState]);

  const runCommand = () => {
    const normalized = command.trim();

    if (!normalized) {
      setConsoleState("empty");
      setOutput("No goal detected. Add the outcome you want before starting an autonomous run.");
      return;
    }

    setConsoleState("loading");
    setOutput("Parsing goal, validating planner actions, and preparing runtime verify checks...");

    window.setTimeout(() => {
      const lower = normalized.toLowerCase();
      if (lower.includes("panic") || lower.includes("force")) {
        setConsoleState("error");
        setOutput("Planner blocked this request. Add constraints and verification criteria.");
        setLastRunAt(
          new Date().toLocaleTimeString([], {
            hour: "2-digit",
            minute: "2-digit",
          }),
        );
        return;
      }

      setConsoleState("success");
      setOutput(
        `Run accepted: "${normalized}". Planner actions are now executing with verify and recovery rails.`,
      );
      setLastRunAt(
        new Date().toLocaleTimeString([], {
          hour: "2-digit",
          minute: "2-digit",
        }),
      );
    }, 900);
  };

  const resetConsole = () => {
    setConsoleState("idle");
    setOutput("Planner idle. Queue a dashboard chat goal to simulate the operator loop.");
  };

  const isRunning = consoleState === "loading";

  return (
    <main id="top" className="relative min-h-screen overflow-x-clip">
      <div className="pointer-events-none absolute inset-0 -z-10">
        <div className="orb orb-one" />
        <div className="orb orb-two" />
        <div className="grid-overlay h-full w-full" />
      </div>

      <header className="sticky top-0 z-40 border-b border-border/70 bg-background/85 backdrop-blur-sm">
        <div className="container flex h-16 items-center justify-between gap-4">
          <a href="#top" className="inline-flex items-center gap-3">
            <img className="brand-logo" src="/branding/logo-primary.svg" alt="lmlang runtime" />
          </a>

          <nav className="hidden items-center gap-6 text-sm text-muted-foreground md:flex">
            <a className="transition-colors hover:text-foreground" href="#architecture">
              Why Runtime Teams Choose It
            </a>
            <a className="transition-colors hover:text-foreground" href="#workflow">
              Workflow
            </a>
            <a className="transition-colors hover:text-foreground" href="#trust">
              Safety
            </a>
          </nav>

          <div className="flex items-center gap-2">
            <a
              className={buttonVariants({ variant: "ghost", size: "sm" })}
              href="https://github.com/snowdamiz/lmlang"
              target="_blank"
              rel="noreferrer"
            >
              GitHub
            </a>
            <Button asChild size="sm">
              <a
                href="https://github.com/snowdamiz/lmlang/blob/main/README.md#quickstart"
                target="_blank"
                rel="noreferrer"
              >
                Start Quickstart
              </a>
            </Button>
          </div>
        </div>
      </header>

      <section className="container pb-20 pt-14 sm:pt-20 lg:pb-24 lg:pt-24">
        <div className="grid gap-10 lg:grid-cols-[1.08fr_0.92fr] lg:items-center">
          <div>
            <span className="section-kicker">
              <Sparkles className="h-3.5 w-3.5" />
              For Teams Evaluating a New AI-Native Language/Runtime
            </span>
            <h1 className="mt-5 max-w-2xl font-display text-4xl font-semibold leading-tight sm:text-5xl lg:text-6xl">
              The first AI-native programming language/runtime for controlled autonomous builds.
            </h1>
            <p className="mt-6 max-w-xl text-base leading-relaxed text-muted-foreground sm:text-lg">
              lmlang treats your program as persistent graph state that agents can inspect, modify,
              verify, and execute directly. The result is autonomous delivery with explicit planner
              contracts, built-in safety rails, and operator-visible recovery paths.
            </p>

            <div className="mt-8 flex flex-col gap-3 sm:flex-row">
              <Button asChild size="lg">
                <a
                  href="https://github.com/snowdamiz/lmlang/blob/main/README.md#quickstart"
                  target="_blank"
                  rel="noreferrer"
                >
                  Start Quickstart
                  <ArrowRight className="h-4 w-4" />
                </a>
              </Button>
              <a
                className={cn(buttonVariants({ variant: "outline", size: "lg" }))}
                href="https://github.com/snowdamiz/lmlang/blob/main/docs/api/operator-endpoints.md"
                target="_blank"
                rel="noreferrer"
              >
                Review Operator API
              </a>
            </div>

          </div>

          <div className="mesh-surface rounded-3xl border border-border/80 p-6 shadow-token sm:p-8">
            <div className="flex items-center justify-between">
              <Badge>Program-as-Data Runtime</Badge>
              <span className="font-mono text-xs text-muted-foreground">state: persistent graph</span>
            </div>

            <div className="mt-6 space-y-4">
              <div className="layer-block semantic-layer">
                <p className="layer-label">Semantic layer</p>
                <div className="node-row">
                  <span className="node-chip">Function: plan_build_goal</span>
                  <span className="node-chip">Contract: safe mutation required</span>
                </div>
              </div>

              <div className="pulse-rail" />

              <div className="layer-block compute-layer">
                <p className="layer-label">Compute layer</p>
                <div className="node-row">
                  <span className="node-chip">Mutate graph</span>
                  <span className="node-chip">Run verify gate</span>
                  <span className="node-chip">Compile dirty set</span>
                </div>
              </div>
            </div>

            <div className="mt-6 grid gap-3 sm:grid-cols-2">
              <div className="rounded-2xl border border-border bg-background/75 p-4">
                <p className="text-xs uppercase tracking-[0.14em] text-muted-foreground">planner</p>
                <p className="mt-2 text-sm font-medium">versioned, validated action schema</p>
              </div>
              <div className="rounded-2xl border border-border bg-background/75 p-4">
                <p className="text-xs uppercase tracking-[0.14em] text-muted-foreground">runtime</p>
                <p className="mt-2 text-sm font-medium">history, checkpoints, undo/redo</p>
              </div>
            </div>
          </div>
        </div>
      </section>

      <section id="architecture" className="container pb-20">
        <div className="architecture-head">
          <span className="section-kicker">
            <BrainCircuit className="h-3.5 w-3.5" />
            Core Differentiators
          </span>
          <h2 className="mt-4 max-w-3xl text-3xl font-semibold leading-tight sm:text-4xl">
            Why runtime teams adopt lmlang for agent-native execution.
          </h2>
          <p className="mt-4 max-w-2xl text-sm leading-relaxed text-muted-foreground sm:text-base">
            Teams evaluating autonomous delivery need a language/runtime that keeps structure,
            control, and recovery in the same execution model.
          </p>
        </div>

        <div className="architecture-timeline">
          {valuePillars.map((pillar, index) => (
            <article key={pillar.title} className="architecture-timeline-item">
              <div className="architecture-timeline-marker" aria-hidden="true">
                <span className="architecture-node-index">{String(index + 1).padStart(2, "0")}</span>
                <span className="architecture-node-icon">
                  <pillar.icon className="h-4 w-4" />
                </span>
              </div>
              <div className="architecture-node-body">
                <h3 className="text-2xl font-semibold leading-tight">{pillar.title}</h3>
                <p className="mt-3 max-w-3xl text-sm leading-relaxed text-muted-foreground sm:text-base">
                  {pillar.description}
                </p>
              </div>
            </article>
          ))}
        </div>
      </section>

      <section id="workflow" className="container pb-20">
        <div className="workflow-shell">
          <div className="workflow-head">
            <span className="section-kicker">
              <Binary className="h-3.5 w-3.5" />
              Operator Workflow
            </span>
            <h2 className="mt-4 max-w-2xl text-3xl font-semibold leading-tight sm:text-4xl">
              Go from first setup to verified execution in four steps.
            </h2>
            <p className="mt-4 max-w-2xl text-sm leading-relaxed text-muted-foreground sm:text-base">
              The dashboard keeps orchestration in one chat loop, so teams can launch, monitor, and
              recover autonomous runs without juggling tools.
            </p>
          </div>

          <ol className="workflow-list">
            {workflowSteps.map((step, index) => (
              <li key={step.title} className="workflow-item">
                <span className="workflow-item-index">{String(index + 1).padStart(2, "0")}</span>
                <h3 className="mt-4 text-2xl font-semibold leading-tight">{step.title}</h3>
                <p className="mt-3 text-sm leading-relaxed text-muted-foreground sm:text-base">
                  {step.description}
                </p>
              </li>
            ))}
          </ol>
        </div>
      </section>

      <section id="trust" className="container pb-16">
        <div className="console-head">
            <span className="section-kicker">
              <TerminalSquare className="h-3.5 w-3.5" />
              Safety and Recovery
            </span>
            <h2 className="mt-4 max-w-3xl text-3xl font-semibold leading-tight sm:text-4xl">
              Run autonomy with guardrails, observability, and rollback.
            </h2>
            <p className="mt-4 max-w-2xl text-sm leading-relaxed text-muted-foreground sm:text-base">
              lmlang keeps verify checks, lock discipline, and persistent runtime history close to
              execution so operators can move fast without blind risk.
            </p>
          </div>

        <div className="grid gap-6 lg:grid-cols-[1.08fr_0.92fr]">
          <Card className="border-primary/20 bg-card/88">
            <CardHeader>
              <Badge variant="muted" className="w-fit">
                <TerminalSquare className="mr-1 h-3.5 w-3.5" />
                Operator Control Preview
              </Badge>
              <CardTitle className="text-2xl">Test the dashboard control loop</CardTitle>
              <CardDescription>
                Simulate the same chat flow operators use in `/dashboard`: define project scope,
                assign an agent, and launch a goal-driven run.
              </CardDescription>
            </CardHeader>
            <CardContent>
              <div className="flex flex-wrap gap-2">
                {commandSuggestions.map((suggestion) => (
                  <button
                    key={suggestion}
                    type="button"
                    onClick={() => {
                      setCommand(suggestion);
                      setConsoleState("idle");
                      setOutput(
                        "Preset queued. Press Run Chat Prompt to simulate planner and verify checks.",
                      );
                    }}
                    className={cn(
                      "rounded-full border px-3 py-1.5 text-left text-xs transition-colors",
                      command === suggestion
                        ? "border-primary/45 bg-primary/10 text-primary"
                        : "border-border bg-background/80 text-muted-foreground hover:border-primary/25 hover:text-foreground",
                    )}
                  >
                    {suggestion}
                  </button>
                ))}
              </div>

              <div className="mt-4 flex flex-col gap-3 sm:flex-row">
                <Input
                  value={command}
                  onChange={(event) => setCommand(event.target.value)}
                  placeholder="Type an orchestration or build prompt..."
                  aria-label="Planner command prompt"
                />
                <Button onClick={runCommand} disabled={isRunning}>
                  {isRunning ? "Running..." : "Run Chat Prompt"}
                </Button>
                <Button variant="outline" onClick={resetConsole}>
                  Reset
                </Button>
              </div>

              <div
                role="status"
                aria-live="polite"
                className={cn(
                  "mt-4 rounded-2xl border p-4 text-sm leading-relaxed transition-colors",
                  toneClassByState[consoleState],
                )}
              >
                <div className="mb-2 flex items-center justify-between gap-3 text-xs font-semibold uppercase tracking-[0.12em]">
                  <span>{statusLabel}</span>
                  <span>{lastRunAt ? `last run ${lastRunAt}` : "no recent run"}</span>
                </div>
                <p>{output}</p>
              </div>
            </CardContent>
          </Card>

          <Card className="bg-card/88">
            <CardHeader>
              <Badge variant="muted" className="w-fit">
                Runtime Trust Signals
              </Badge>
              <CardTitle className="text-2xl">Runtime rails for reliable autonomous delivery</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              {trustRows.map((row) => (
                <div key={row.title} className="telemetry-row">
                  <div className="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-secondary text-secondary-foreground">
                    <row.icon className="h-[18px] w-[18px]" />
                  </div>
                  <div>
                    <h3 className="font-semibold">{row.title}</h3>
                    <p className="mt-1 text-sm leading-relaxed text-muted-foreground">
                      {row.description}
                    </p>
                  </div>
                </div>
              ))}
            </CardContent>
          </Card>
        </div>
      </section>

      <section className="container pb-20">
        <div className="cta-panel rounded-3xl p-8 text-primary-foreground shadow-token sm:p-10">
          <span className="inline-flex rounded-full border border-primary-foreground/35 bg-primary-foreground/15 px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.14em]">
            Ready to Evaluate
          </span>
          <h2 className="mt-4 max-w-3xl text-3xl font-semibold leading-tight sm:text-4xl">
            Start Quickstart and evaluate lmlang in one operator session.
          </h2>
          <p className="mt-4 max-w-2xl text-sm leading-relaxed text-primary-foreground/90 sm:text-base">
            Launch `/dashboard`, configure your agent once, and move from build goal to verified
            execution with inspectable planner and runtime diagnostics.
          </p>
          <div className="mt-7 flex flex-col gap-3 sm:flex-row">
            <Button asChild variant="secondary" size="lg">
              <a
                href="https://github.com/snowdamiz/lmlang/blob/main/README.md#quickstart"
                target="_blank"
                rel="noreferrer"
              >
                Start Quickstart
                <ArrowRight className="h-4 w-4" />
              </a>
            </Button>
            <a
              className={cn(
                buttonVariants({ variant: "outline", size: "lg" }),
                "border-primary-foreground/35 bg-transparent text-primary-foreground hover:bg-primary-foreground/15 hover:text-primary-foreground",
              )}
              href="https://github.com/snowdamiz/lmlang/blob/main/docs/api/operator-endpoints.md"
              target="_blank"
              rel="noreferrer"
            >
              Review Operator API
            </a>
          </div>
        </div>
      </section>

      <footer className="container pb-8">
        <div className="flex flex-col gap-2 border-t border-border/70 pt-5 text-xs text-muted-foreground sm:flex-row sm:items-center sm:justify-between">
          <p>lmlang landing page built with React 19, Vite, Tailwind, and shadcn/ui primitives.</p>
          <p>Static deploy ready via `npm run build`.</p>
        </div>
      </footer>
    </main>
  );
}

export default App;
