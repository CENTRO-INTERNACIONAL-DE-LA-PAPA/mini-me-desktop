# Mini-Me

<p align="center">
  <img src="images/mini_me.png" alt="Mini-Me" width="480" />
</p>

> *"It's me… but smaller, and it analyzes your data."* — every researcher, eventually.

**Mini-Me** is a multi-agent research workbench: a coordinator agent and a
team of specialized subagents that walk you through the entire data
life cycle — from literature review to dataset discovery, cleaning,
exploratory and diagnostic analysis, predictive modeling, and a polished
publication-ready report.

The name is a nod to *Austin Powers*: a smaller, dedicated version of you
that handles the busywork of being a data scientist so you can focus on
the science. The aspiration is that anyone — agronomists, geneticists,
breeders, social scientists — can fork this project and shape their own
mini-me around their domain skills.

## What's in the team

A coordinator delegates work to seven specialized subagents:

- **Academic Researcher** — peer-reviewed literature search via Asta
- **Dataverse Explorer** — discovers and inspects datasets in CIP Dataverse
- **Data Cleaner** — schema validation, harmonization with AGROVOC + Crop Ontology
- **Exploratory Data Analysis** — descriptive stats and pattern surfacing (*what happened?*)
- **Diagnostic Analytics** — regression, confounding, group comparisons (*why?*)
- **Predictive Analytics** — model selection and training (*what will happen?*)
- **Report Writer** — synthesizes findings into a markdown report and a styled PDF

Knowledge sources wired in: **Asta**, **CIP Dataverse**, **AGROVOC**,
**Crop Ontology**, plus an isolated **Daytona** sandbox per conversation
for safe code execution.

## Quick start

### 1. Clone

```bash
git clone https://github.com/CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me.git
cd Mini-Me
```

### 2. Backend (Python 3.12+)

```bash
# Install uv if you don't have it: https://docs.astral.sh/uv/
uv sync --extra dev
cp .env.example .env
# Edit .env and add your keys
```

### 3. Frontend (Node 20+)

```bash
cd frontend
npm install
```

### 4. Run

```bash
# Terminal 1 — LangGraph backend
uv run langgraph dev

# Terminal 2 — Vite frontend
cd frontend && npm run dev
```

Open the URL Vite prints (usually `http://localhost:5173`).

## Architecture

- **Backend** — a `backend/` Python package built on
  [LangGraph](https://langchain-ai.github.io/langgraph/) + the
  [`deepagents`](https://github.com/hwchase17/deepagents) framework. The
  coordinator and subagent graph lives in `backend/agent.py` +
  `backend/subagents.py`; custom HTTP routes in `backend/routes/` stream
  artifacts and render PDFs in-process via `pypandoc-binary` + `typst`.
  `langgraph.json` wires the deploy entry points.
- **Frontend** — React 19 + Vite, single-page UI (`frontend/src/`,
  `components/` + `lib/`). Citrus × Berry palette, accessible markdown
  rendering, throttled token streaming, lightboxes for images and reports.
- **Skills** — domain knowledge lives in `skills/` (one folder per
  capability: research, dataverse, data_cleaning, EDA, diagnostic,
  predictive, report writing, visualization). Subagents read these on
  demand.
- **Sandbox** — each conversation gets a fresh
  [LangSmith Sandbox](https://docs.smith.langchain.com/) so generated
  Python and shell code stays isolated (needs `LANGSMITH_API_KEY`). The
  provider is configurable via `deepagents.toml` — Daytona, Modal, and
  Runloop are also supported.

## Deployment

Mini-Me ships as two independent pieces that talk over HTTP:

| Piece | What it is | Reference target |
|-------|------------|------------------|
| **Backend** | the LangGraph graph + custom routes (`langgraph.json`) | **LangGraph Platform** (in LangSmith) |
| **Frontend** | a static React SPA (Vite build) | **Amazon S3 + CloudFront** |

Either target is swappable — self-host the backend as a Docker image, or
serve the frontend from any static host (Netlify, Cloudflare Pages, nginx).
The only contract between them is the **backend URL** and **CORS**.

### 1. Backend → LangGraph Platform

The repo is deploy-ready: `langgraph.json` already points at
`backend/agent.py:agent` (graph), `backend/routes/__init__.py:app` (HTTP
routes), and `backend/auth.py:auth` (auth).

1. Push the repo to GitHub.
2. In **LangSmith → Deployments → + New Deployment**, connect this repo and
   branch. LangGraph Platform auto-detects `langgraph.json` and builds from
   `pyproject.toml`.
3. Set the environment variables (same keys as `.env.example`) in the
   deployment's **Environment** settings — they are stored by the platform,
   never committed:
   - **Required:** `OPENAI_API_KEY`, `LANGSMITH_API_KEY`, `ASTA_API_KEY`
   - **Auth (production):** `WORKOS_CLIENT_ID`, `WORKOS_API_KEY`,
     `AUTH_ALLOWED_EMAIL_DOMAINS`, and `DEEP_ATD_RUNTIME_MODE=production`
4. Deploy, then copy the deployment's base URL — it becomes the frontend's
   `VITE_LANGGRAPH_API_URL`.
5. **Allow your frontend origin through CORS:** edit `cors.allow_origins`
   in `langgraph.json` to include your CloudFront/custom domain, commit, and
   redeploy.

> **Self-host alternative:** `uv run langgraph build -t mini-me` produces a
> Docker image you can run on any host; supply the same environment variables
> at runtime.

### 2. Frontend → S3 + CloudFront (GitHub Actions)

`.github/workflows/deploy-frontend.yml` builds and ships the SPA on every
push to `main` that touches `frontend/**` (and via **Run workflow** on
demand). It builds with Vite, `aws s3 sync`s `dist/` to your bucket
(long-cache for hashed assets, no-cache for `index.html`), and invalidates
CloudFront so users pick up new builds immediately.

**One-time AWS setup:**

- An **S3 bucket** to hold the static build.
- A **CloudFront distribution** in front of it with an **SPA rewrite**:
  map `403`/`404` responses to `/index.html` with a `200` so client-side
  routes resolve.

**GitHub → Settings → Secrets and variables → Actions**, add:

| Secret | Purpose |
|--------|---------|
| `VITE_LANGGRAPH_API_URL` | backend deployment URL from step 1 |
| `VITE_WORKOS_CLIENT_ID` | WorkOS AuthKit client id (leave blank to disable sign-in) |
| `VITE_WORKOS_REDIRECT_URI` | post-login redirect, e.g. `https://your-domain/` |
| `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` | IAM creds with `s3:PutObject` + `cloudfront:CreateInvalidation` |
| `AWS_REGION` | the bucket's region |
| `S3_BUCKET` | bucket name |
| `CLOUDFRONT_DISTRIBUTION_ID` | distribution id |

(`VITE_LANGGRAPH_ASSISTANT_ID` defaults to `agent` and is set inline in the
workflow.)

### 3. Wire the two together

Three values must agree across the deployments:

1. **Backend URL** → the frontend's `VITE_LANGGRAPH_API_URL`.
2. **Frontend origin** → the backend's `cors.allow_origins` in `langgraph.json`.
3. **WorkOS** → the same redirect URI in `VITE_WORKOS_REDIRECT_URI` *and* your
   WorkOS app's allowed redirect URIs, and the email allowlist aligned
   between `VITE_AUTH_ALLOWED_EMAIL_DOMAINS` (frontend gate) and
   `AUTH_ALLOWED_EMAIL_DOMAINS` (backend enforcement).

### Authentication is optional

With no WorkOS configured and `DEEP_ATD_RUNTIME_MODE` unset (or
`development`), the backend falls back to a stub `local-user` identity and
the frontend skips the sign-in gate — fine for local or trusted-network
deployments. For anything public, configure WorkOS so requests are
authenticated and every thread/store resource is scoped to its owner.

## Roll your own Mini-Me

The whole point is forkability. To shape Mini-Me to your domain:

1. **Edit `skills/`** — drop in your standard operating procedures, your
   citation rules, your preferred plotting style. Each `SKILL.md` is read
   by the relevant subagent at runtime.
2. **Adjust the subagent roster** in `backend/subagents.py` — add new ones,
   remove ones you don't need, or rewire MCP tools. The coordinator prompt
   lives in `backend/prompts.py`.
3. **Swap the data sources** — MCP servers are declared in the
   `MCP_SERVER_CONFIGS` dict in `backend/mcp_tools.py`. Add, remove,
   or replace entries there to wire in whatever data sources your field uses.
4. **Rebrand the UI** — three edit points, by concern:
   - **Name, tagline, logo, About copy** → `frontend/src/branding.ts`. Change
     `appName` once and it flows to the top bar, sign-in gate, About modal, and
     browser tab. Set `logo: { src: "/your-logo.svg" }` (a file in
     `frontend/public/`) to swap the default gradient mark for an image.
   - **Colors** → the `:root` / `.dark` token blocks at the top of
     `frontend/src/styles.css`. `--accent` (primary) and `--berry` (secondary)
     drive the accents, the logo, **and** the animated background — change those
     two and everything re-tints in both light and dark.
   - **Background animation** → open [`docs/backgrounds.html`](docs/backgrounds.html)
     in a browser to preview 15 ready-made designs (watercolor, silk, ink, mist,
     marble, color-field…). To switch, paste your chosen design's markup into
     `frontend/src/components/Background.tsx` and its CSS into `styles.css`.
     They're palette-agnostic, so they adopt your `--accent` / `--berry` colors
     automatically.

#### Example: swap the default blobs for falling petals

Say you previewed `docs/backgrounds.html` and liked design **#09 "Pétalos"**.
Two edits:

**1. Add its CSS** to the bottom of `frontend/src/styles.css`:

```css
.petal {
  position: absolute;
  top: -12%;
  opacity: 0.55;
  background: linear-gradient(
    135deg,
    color-mix(in srgb, var(--rose) 70%, transparent),
    color-mix(in srgb, var(--berry) 50%, transparent)
  );
  border-radius: 80% 0 80% 0;
}
@keyframes fall {
  0%   { transform: translateY(-20px) translateX(0)    rotate(0deg); }
  50%  { transform: translateY(160px) translateX(18px)  rotate(80deg); }
  100% { transform: translateY(360px) translateX(-10px) rotate(170deg); }
}
```

**2. Replace the body of `frontend/src/components/Background.tsx`** — most
designs are pure CSS (just swap the `<span>`s), but Pétalos generates its
petals, so here's the React version:

```tsx
import { useMemo } from "react";

export function Background() {
  // Six petals with randomized size, position, and fall timing (computed once
  // so they don't regenerate on every render).
  const petals = useMemo(
    () =>
      Array.from({ length: 6 }, () => {
        const w = 10 + Math.random() * 14;
        const dur = 13 + Math.random() * 9;
        return {
          width: `${w}px`,
          height: `${w * 0.7}px`,
          left: `${8 + Math.random() * 84}%`,
          opacity: 0.3 + Math.random() * 0.35,
          animation: `fall ${dur.toFixed(0)}s linear -${(Math.random() * dur).toFixed(0)}s infinite`,
        };
      }),
    [],
  );

  return (
    <div className="ambient" aria-hidden="true">
      {petals.map((style, i) => (
        <span key={i} className="petal" style={style} />
      ))}
    </div>
  );
}
```

Reload — petals now drift down in your palette's `--rose` / `--berry`. The 13
CSS-only designs (watercolor, silk, mist, marble…) are simpler still: paste
their CSS and replace the `<span>`s with the design's static markup, no JS.

## Acknowledgements

Academic literature search is powered by **Asta**, the scientific research
agent suite from the **Allen Institute for AI**. If your work uses output
produced with Asta, please cite the AstaBench paper:

> *AstaBench: Rigorous Benchmarking of AI Agents with a Scientific Research Suite.*
> arXiv:2510.21652 — <https://arxiv.org/abs/2510.21652>

## Attribution

A work produced by the **International Potato Center, Area of Work 3
(AoW3)** under the **CGIAR Initiative on Digital Transformation**.

## License

[Creative Commons Attribution 4.0 International (CC BY 4.0)](LICENSE)

You are free to share and adapt this work for any purpose, including
commercially, as long as you give appropriate credit to the
International Potato Center (CIP).
