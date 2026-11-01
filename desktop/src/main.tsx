// Desktop UI: the daemon's applications, their releases and backups, recent jobs and logs, and the
// actions that queue jobs.
//
// Layout: a defuss-shadcn Border Layout. West is the Sidebar (resizable, foldable), north the
// Toolbar, east the selected application, and the center splits into the applications Data Grid
// above and the activity below (jobs Data Grid beside a Virtual List of log lines). The shell is
// mounted once; afterwards only app-owned containers are morphed, and the grids and the log list
// get their records through their component APIs.
//
// The page holds no state of its own beyond the last poll. Jobs run inside the daemon and outlive
// this window (ARCH.md, "One daemon, thin clients"), so every poll replaces `apps` and `jobs` and
// repaints from them.
import { df$, ui, type Rec, type Stateful } from './df';
import { api, active, type App, type Backup, type Job, type Release } from './api';
import './app.css';

let apps: App[] = [];
let jobs: Job[] = [];
let selected: string | null = null;
let online = false;
// `pending` blocks a second submit of the create form; `refreshing` stops the timer, an action and
// the Reconnect button from running overlapping polls; `failures` drives the reconnect backoff.
let pending = false;
let refreshing = false;
let failures = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
// Releases and backups of the selected app, the tree row picked in them, and the job states they
// were loaded at: a finished build or backup of that app reloads them.
let history: Rec[] = [];
let historyPick: Rec | undefined;
let historyKey = '';
// The log panel: every line of the last log opened, the indexes the filter keeps, and the record
// behind it for the Details dialog.
let logLines: string[] = [];
let logShown: number[] = [];
let logDetails: unknown;

const $ = (id: string) => document.getElementById(id)!;
const pretty = (v: unknown) => (typeof v === 'string' ? v : JSON.stringify(v, null, 2));
const date = (s: number) => new Date(s * 1000).toLocaleString();
const dialog = (id: string) => $(id) as HTMLDialogElement;
const busy = (name: string) => jobs.some((j) => j.app === name && active(j));
const failed = (e: unknown) => message(String(e), true);

function message(text: string, error = false) {
  $('notice').textContent = text;
  $('notice').className = error ? 'notice error' : 'notice';
}
function details(v: unknown) {
  $('detail-output').textContent = pretty(v);
  dialog('detail').showModal();
}

// Queues a job, then polls at once, so the new job and the disabled buttons show without waiting
// for the next poll.
async function action(name: string, kind: string, extra: Record<string, unknown> = {}) {
  try {
    const j = await api<Job>('POST', `/v1/apps/${name}/actions`, { action: kind, ...extra });
    message(`${kind} queued · ${j.id}`);
    await refresh();
  } catch (e) {
    failed(e);
  }
}

// Connected: poll every 3 s while visible. Not connected: retry after 1, 2, 4 … 30 s, so a daemon
// started later is found without a click and a stopped one is not polled every 3 s. Reconnect
// retries at once and resets the backoff.
const POLL_MS = 3000,
  RETRY_MS = 1000,
  RETRY_MAX_MS = 30000;
function schedule() {
  clearTimeout(timer);
  const ms = online ? POLL_MS : Math.min(RETRY_MS * 2 ** Math.max(failures - 1, 0), RETRY_MAX_MS);
  timer = setTimeout(() => {
    if (online && document.hidden) schedule();
    else void refresh();
  }, ms);
  return ms;
}
function connection(text: string, up: boolean) {
  $('connection').textContent = text;
  $('connection').className = up ? 'connection online' : 'connection';
  $('reconnect').hidden = up;
}
async function refresh() {
  if (refreshing) return;
  refreshing = true;
  try {
    [apps, jobs] = await Promise.all([
      api<App[]>('GET', '/v1/apps'),
      api<Job[]>('GET', '/v1/jobs'),
    ]);
    if (!online) message('Connected to the local daemon.');
    online = true;
    failures = 0;
    connection('Daemon connected', true);
    schedule();
    paint();
  } catch (e) {
    online = false;
    failures++;
    const s = schedule() / 1000;
    connection('Daemon unavailable', false);
    message(
      `${e}. Start the daemon (make start, or the installed service). Retrying automatically: attempt ${failures}, next in ${s} s.`,
      true,
    );
  } finally {
    refreshing = false;
  }
}
function reconnect() {
  failures = 0;
  connection('Connecting…', false);
  void refresh();
}

// Grid cells that show a state as a badge; `el` is a fresh cell span each time.
function badge(text: string, variant: string) {
  const b = document.createElement('span');
  b.className = 'badge';
  b.dataset.variant = variant;
  b.textContent = text;
  return b;
}
const STATE_VARIANT: Record<string, string> = {
  running: 'default',
  succeeded: 'secondary',
  failed: 'destructive',
  interrupted: 'destructive',
};
const stateCell = (el: HTMLElement, r: Rec) =>
  el.append(badge(String(r.state), STATE_VARIANT[String(r.state)] ?? 'outline'));

function paint() {
  $('count').textContent = String(apps.length);
  $('running-count').textContent = String(apps.filter((a) => a.desired_running).length);
  $('job-count').textContent = String(jobs.filter(active).length);
  ui.dataGrid.setSource(
    $('apps'),
    apps.map((a) => ({
      name: a.spec.name,
      state: a.desired_running ? 'running' : 'stopped',
      release: a.active_release?.slice(0, 8) || 'none',
      git_ref: a.spec.git_ref,
      cpus: a.spec.cpus,
      memory_mib: a.spec.memory_mib,
    })),
    { idField: 'name', cells: { state: stateCell } },
  );
  ui.dataGrid.setSource(
    $('jobs'),
    jobs.map((j) => ({
      id: j.id,
      app: j.app,
      action: j.action.action,
      state: j.state,
      created: j.created_at ? date(j.created_at) : '',
    })),
    { idField: 'id', cells: { state: stateCell } },
  );
  bind();
  // The first app is selected on start, and again when the selected one was deleted.
  if (apps.length && !apps.some((a) => a.spec.name === selected)) select(apps[0].spec.name);
  else renderApp();
}

function select(name: string | null) {
  selected = name;
  const grid = $('apps') as Stateful;
  if (name && grid.api && !grid.store.value.config.selected?.includes(name))
    grid.api.setState('default', { selected: [name] });
  historyKey = '';
  renderApp();
}

// The grids' selection lives in each grid's store; subscribing needs the component initialized,
// which its MutationObserver does after the shell is mounted. paint() retries until it is.
// `jobPick` is the job last picked in the jobs grid; its log opens only when the pick changes.
let bound = false;
let jobPick: unknown;
function bind() {
  const [appsGrid, jobsGrid, historyGrid] = ['apps', 'jobs', 'history'].map(
    (id) => $(id) as Stateful,
  );
  if (bound || !appsGrid.store || !jobsGrid.store || !historyGrid.store) return;
  bound = true;
  appsGrid.store.subscribe(({ config }) => {
    const name = (config.selected?.[0] as string | undefined) ?? null;
    if (name && name !== selected) select(name);
  });
  jobsGrid.store.subscribe(({ config }) => {
    const id = config.selected?.[0];
    if (id === jobPick) return;
    jobPick = id;
    const job = jobs.find((j) => j.id === id);
    if (job) void showJob(job);
  });
  historyGrid.store.subscribe(({ config }) => {
    historyPick = history.find((r) => r.id === config.selected?.[0]);
    $('deploy-release').toggleAttribute('disabled', historyPick?.kind !== 'release');
    $('restore-backup').toggleAttribute('disabled', historyPick?.kind !== 'backup');
  });
}

function renderApp() {
  const app = apps.find((a) => a.spec.name === selected);
  df$('#application').morph(<AppPanel app={app} />);
  void loadHistory(app);
}

function AppPanel({ app }: { app: App | undefined }) {
  if (!app)
    return (
      <div class="welcome">
        <span class="hero-icon">◈</span>
        <h2>Your backend, locally.</h2>
        <p>Build a Bun repository into a distroless image and run it in its own Linux machine.</p>
        <button class="btn" type="button" onClick={() => dialog('create').showModal()}>
          Add application
        </button>
      </div>
    );
  const name = app.spec.name;
  const disabled = busy(name);
  return (
    <div class="app-panel">
      <div class="panel-heading">
        <div>
          <div class="eyebrow">APPLICATION</div>
          <h2>{name}</h2>
        </div>
        <span class="badge" data-variant={app.desired_running ? 'default' : 'outline'}>
          {app.desired_running ? 'Desired: running' : 'Desired: stopped'}
        </span>
      </div>
      <p class="repository">{app.spec.repository}</p>
      <div class="metadata">
        <span>
          Ref <b>{app.spec.git_ref}</b>
        </span>
        <span>
          Release <b>{app.active_release?.slice(0, 8) || 'None'}</b>
        </span>
        <span>
          Storage <b>Guest-local ext4</b>
        </span>
      </div>
      {app.last_error ? <pre class="inline-error">{app.last_error}</pre> : null}
      <div class="actions">
        <button class="btn" type="button" disabled={disabled} onClick={() => action(name, 'build')}>
          Build release
        </button>
        <button
          class="btn"
          data-variant="outline"
          type="button"
          disabled={disabled || !app.active_release}
          onClick={() => action(name, app.desired_running ? 'stop' : 'start')}
        >
          {app.desired_running ? 'Stop' : 'Start'}
        </button>
        <button
          class="btn"
          data-variant="outline"
          type="button"
          disabled={disabled || !app.previous_release}
          onClick={() => action(name, 'rollback')}
        >
          Rollback
        </button>
        <button
          class="btn"
          data-variant="outline"
          type="button"
          disabled={disabled || !app.active_release}
          onClick={() => action(name, 'backup')}
        >
          Back up data
        </button>
      </div>
      <div class="links">
        <button class="btn" data-variant="ghost" type="button" onClick={() => liveStatus(name)}>
          Live status
        </button>
        <button class="btn" data-variant="ghost" type="button" onClick={() => appLogs(name)}>
          Logs
        </button>
        <button
          class="btn"
          data-variant="ghost"
          data-tone="destructive"
          type="button"
          disabled={disabled}
          onClick={() => deleteApp(name)}
        >
          Delete machine
        </button>
      </div>
      <div class="storage">
        <h3>Host shares</h3>
        <code>{app.spec.mounts}</code>
        <p>input · read-only / output · writable / etc · read-only</p>
        <p>
          Application data persists inside the VM. Backups export <code>/app/var</code>.
        </p>
      </div>
    </div>
  );
}

// The releases and backups of the selected app as a tree grid: two group rows with the records
// below them. Loaded when the selection changes or a job of that app changes state, not on every
// poll, so an idle window sends two requests every 3 s, not four.
async function loadHistory(app: App | undefined) {
  const key = app
    ? `${app.spec.name}|${jobs
        .filter((j) => j.app === app.spec.name)
        .map((j) => j.id + j.state)
        .join()}`
    : '';
  if (key === historyKey) return;
  historyKey = key;
  let rows: Rec[] = [];
  if (app) {
    const name = app.spec.name;
    try {
      const [releases, backups] = await Promise.all([
        api<Release[]>('GET', `/v1/apps/${name}/releases`),
        api<Backup[]>('GET', `/v1/apps/${name}/backups`),
      ]);
      rows = [
        { id: 'releases', parent: null, kind: 'group', name: `Releases (${releases.length})` },
        ...releases.map((r) => ({
          id: r.id,
          parent: 'releases',
          kind: 'release',
          name: r.id,
          detail: `${r.id === app.active_release ? 'active · ' : ''}${r.commit.slice(0, 12)} · ${date(r.created_at)}`,
        })),
        { id: 'backups', parent: null, kind: 'group', name: `Backups (${backups.length})` },
        ...backups.map((b) => ({
          id: b.id,
          parent: 'backups',
          kind: 'backup',
          name: b.id,
          detail: `${(b.bytes / 1048576).toFixed(1)} MiB · ${date(b.created_at)}`,
        })),
      ];
    } catch (e) {
      failed(e);
    }
    // A newer selection or job change started its own load while this one waited.
    if (key !== historyKey) return;
  }
  history = rows;
  ui.dataGrid.setSource($('history'), rows, { idField: 'id' });
  ui.dataGrid.expandAll($('history'));
}

function deployPicked() {
  if (selected && historyPick?.kind === 'release')
    void action(selected, 'deploy', { release: historyPick.id });
}
function restorePicked() {
  if (
    selected &&
    historyPick?.kind === 'backup' &&
    confirm('Replace current application data with this backup? A safety backup is made first.')
  )
    void action(selected, 'restore', { backup: historyPick.id });
}

// The typed name travels as `confirm`; the daemon refuses a delete whose confirmation differs from
// the name (src/model.rs Action::validate), so the check holds for every client, not only here.
function deleteApp(name: string) {
  const entered = prompt(
    `This permanently deletes ${name}'s VM and guest data. Host shares and backups remain. Type ${name} to continue.`,
  );
  if (entered === name) void action(name, 'delete', { confirm: entered });
}

async function liveStatus(name: string) {
  try {
    details(await api('GET', `/v1/apps/${name}`));
  } catch (e) {
    failed(e);
  }
}
async function diagnostics() {
  try {
    details(await api('GET', '/v1/doctor'));
  } catch (e) {
    failed(e);
  }
}

// Logs go to a Virtual List: only the rows on screen exist, so a long job log stays cheap, and
// every line is set as text, never parsed as HTML.
function showLog(title: string, text: string, record: unknown) {
  logLines = text.replace(/\n$/, '').split('\n');
  logDetails = record;
  $('log-title').textContent = title;
  $('log-title').title = title;
  $('log-details').toggleAttribute('disabled', record === undefined);
  filterLog();
}
function filterLog() {
  const needle = ($('log-filter') as HTMLInputElement).value.toLowerCase();
  logShown = [];
  logLines.forEach((line, i) => {
    if (!needle || line.toLowerCase().includes(needle)) logShown.push(i);
  });
  const list = $('log') as Stateful;
  ui.virtualList.setData(list, logShown.length, (row, i) => {
    row.textContent = logLines[logShown[i]] ?? '';
  });
  // Open at the newest line, where a job log ends.
  if (logShown.length && list.api) list.api.setState('default', { index: logShown.length - 1 });
}
async function appLogs(name: string) {
  try {
    const text = await api<string>('GET', `/v1/apps/${name}/logs`);
    showLog(`${name}: last 200 lines`, text, undefined);
  } catch (e) {
    failed(e);
  }
}
async function showJob(job: Job) {
  try {
    const log = await api<string>('GET', `/v1/jobs/${job.id}/log`);
    showLog(`Job ${job.action.action} · ${job.app}`, log, job);
  } catch (e) {
    failed(e);
  }
}

async function create(event: Event) {
  event.preventDefault();
  if (pending) return;
  pending = true;
  const f = new FormData($('create-form') as HTMLFormElement);
  const value = (k: string) => String(f.get(k) || '');
  try {
    const spec = {
      name: value('name'),
      repository: value('repository'),
      git_ref: value('git_ref') || 'HEAD',
      subdirectory: value('subdirectory'),
      mounts: value('mounts'),
      cpus: Number(value('cpus')),
      memory_mib: Number(value('memory')),
      // The form does not offer disk size or stop timeout; these are the spec defaults in
      // src/model.rs.
      storage_gib: 20,
      ports: [{ host: Number(value('host_port')), guest: Number(value('guest_port')) }],
      env: JSON.parse(value('env') || '{}'),
      build: f.has('build'),
      health: { port: Number(value('guest_port')), path: value('health') || '/health' },
      stop_timeout_seconds: 30,
    };
    await api('POST', '/v1/apps', spec);
    dialog('create').close();
    message(`Added ${spec.name}. Build its first release.`);
    await refresh();
    select(spec.name);
  } catch (e) {
    $('form-error').textContent = String(e);
  } finally {
    pending = false;
  }
}

type Column = [field: string, label: string, width: string, filter?: string];
// A data grid's head: the component builds the filter row, the row pool and the body from it.
function Grid(props: {
  id: string;
  label: string;
  empty: string;
  parent?: string;
  columns: Column[];
}) {
  // data-persist="none": the rows come from the daemon on every poll, so no view is kept in
  // session storage between windows.
  return (
    <div
      class="data-grid"
      id={props.id}
      aria-label={props.label}
      data-select="single"
      data-persist="none"
      data-parent-field={props.parent}
      data-empty-text={props.empty}
    >
      <div class="data-grid-viewport">
        <div class="data-grid-head">
          <div class="data-grid-row">
            {props.columns.map(([field, label, width, filter]) => (
              <div
                class="data-grid-header"
                data-field={field}
                data-width={width}
                data-filter={filter}
              >
                {label}
              </div>
            ))}
          </div>
        </div>
        <div class="data-grid-body"></div>
      </div>
    </div>
  );
}

function Shell() {
  return (
    <div
      class="border-layout"
      id="frame"
      data-dominant="we"
      data-frame="none"
      data-center-min="320"
    >
      <div class="resizer border-layout-west" data-min="180" data-max="420" data-collapsible>
        {/* data-state-name="default": the Sidebar folds itself when its parent is narrower than
            384 px, and inside the resizer its parent is its own region. */}
        <aside
          class="app-sidebar"
          id="nav"
          data-state-name="default"
          aria-label="Navigation"
          style={{ width: '15rem' }}
        >
          <div class="sidebar-header">
            <span class="brand">
              ◈ defuss<span class="brand-light">container</span>
            </span>
          </div>
          <div class="sidebar-content">
            <nav class="sidebar-nav">
              <a class="sidebar-link" aria-current="page">
                <span>Applications</span>
                <span class="sidebar-badge" id="count">
                  0
                </span>
              </a>
              <button class="sidebar-link" type="button" onClick={diagnostics}>
                <span>Runtime diagnostics</span>
              </button>
            </nav>
          </div>
          <div class="sidebar-footer">
            <span id="connection" class="connection">
              Connecting…
            </span>
            <small>v0.1.0 · Bun 1.4.4 · smolvm 1.32.4</small>
          </div>
        </aside>
      </div>
      <header class="border-layout-north topbar">
        <div class="toolbar" role="toolbar" aria-label="Applications">
          <button
            class="btn"
            data-variant="ghost"
            data-size="icon"
            type="button"
            aria-label="Toggle sidebar"
            onClick={() => ui.borderLayout.toggle($('frame'), 'west')}
          >
            ☰
          </button>
          <div class="title">
            <div class="eyebrow">RUNTIME CONTROL</div>
            <h1>Applications</h1>
          </div>
          <span class="stat">
            Desired running <b id="running-count">0</b>
          </span>
          <span class="stat">
            Active operations <b id="job-count">0</b>
          </span>
          <span class="spacer"></span>
          <button
            id="reconnect"
            class="btn"
            data-variant="outline"
            type="button"
            onClick={reconnect}
          >
            Reconnect
          </button>
          <button class="btn" type="button" onClick={() => dialog('create').showModal()}>
            + Add application
          </button>
        </div>
      </header>
      <main class="border-layout-center">
        <div class="border-layout" id="workspace" data-frame="none" data-center-min="120">
          <div class="resizer border-layout-north" data-min="140">
            <section class="pane" style={{ height: '45%' }}>
              <div id="notice" class="notice" role="status" aria-live="polite"></div>
              <Grid
                id="apps"
                label="Applications"
                empty="No applications yet. Add a repository to begin."
                columns={[
                  ['name', 'Name', 'minmax(9rem, 1fr)', 'text'],
                  ['state', 'Desired', '8rem', 'select'],
                  ['release', 'Release', '7rem'],
                  ['git_ref', 'Ref', '7rem'],
                  ['cpus', 'vCPU', '5rem'],
                  ['memory_mib', 'MiB', '6rem'],
                ]}
              />
            </section>
          </div>
          <div class="border-layout-center">
            <div class="border-layout" id="activity" data-frame="none" data-center-min="160">
              <div class="resizer border-layout-west" data-min="240">
                <section class="pane" style={{ width: '48%' }}>
                  <div class="section-heading">
                    <h3>Recent activity</h3>
                    <span>Operations continue when this window closes</span>
                  </div>
                  <Grid
                    id="jobs"
                    label="Jobs"
                    empty="Builds, deployments and backups appear here."
                    columns={[
                      ['state', 'State', '7rem', 'select'],
                      ['app', 'App', '6rem', 'select'],
                      ['action', 'Action', '6rem', 'select'],
                      ['created', 'Queued', 'minmax(8rem, 1fr)'],
                    ]}
                  />
                </section>
              </div>
              <section class="border-layout-center pane">
                <div class="section-heading">
                  <h3 id="log-title">Logs</h3>
                  <input
                    class="input"
                    id="log-filter"
                    type="search"
                    placeholder="Filter lines"
                    aria-label="Filter log lines"
                    onInput={filterLog}
                  />
                  <button
                    class="btn"
                    data-variant="outline"
                    data-size="sm"
                    id="log-details"
                    type="button"
                    disabled
                    onClick={() => details(logDetails)}
                  >
                    Details
                  </button>
                </div>
                <div class="virtual-list" id="log" aria-label="Log lines"></div>
              </section>
            </div>
          </div>
        </div>
      </main>
      <div class="resizer border-layout-east" data-min="280" data-max="640" data-collapsible>
        <aside class="detail" aria-label="Selected application" style={{ width: '28rem' }}>
          <section id="application"></section>
          <section class="history pane">
            <div class="section-heading">
              <h3>Releases and backups</h3>
              <button
                class="btn"
                data-size="sm"
                id="deploy-release"
                type="button"
                disabled
                onClick={deployPicked}
              >
                Deploy release
              </button>
              <button
                class="btn"
                data-variant="outline"
                data-size="sm"
                id="restore-backup"
                type="button"
                disabled
                onClick={restorePicked}
              >
                Restore backup
              </button>
            </div>
            <Grid
              id="history"
              label="Releases and backups"
              empty="Select an application."
              parent="parent"
              columns={[
                ['name', 'Name', '15rem'],
                ['detail', 'Detail', 'minmax(10rem, 1fr)'],
              ]}
            />
          </section>
        </aside>
      </div>
      <CreateDialog />
      <dialog id="detail" class="dialog" aria-labelledby="detail-title">
        <div class="dialog-content">
          <div class="dialog-header">
            <h2 class="dialog-title" id="detail-title">
              Details
            </h2>
          </div>
          <pre id="detail-output"></pre>
          <div class="dialog-footer">
            <button
              class="btn"
              data-variant="outline"
              type="button"
              onClick={() => dialog('detail').close()}
            >
              Close
            </button>
          </div>
        </div>
      </dialog>
    </div>
  );
}

// VERIFIED: browsers compile `pattern` with the `v` flag, where an unescaped trailing `-` in a class
// is a syntax error that silently disables the check; the name pattern escapes it
// (tests/ui.test.mjs).
function CreateDialog() {
  const field = (label: string, input: unknown, wide = false) => (
    <label class={wide ? 'label wide' : 'label'}>
      {label}
      {input}
    </label>
  );
  return (
    <dialog id="create" class="dialog" aria-labelledby="create-title">
      <form id="create-form" class="dialog-content" onSubmit={create}>
        <div class="dialog-header">
          <h2 class="dialog-title" id="create-title">
            Add application
          </h2>
          <p class="dialog-description">
            Repository must contain a Bun lockfile and start / stop scripts.
          </p>
        </div>
        <div class="form-grid">
          {field(
            'Name',
            <input
              class="input"
              name="name"
              required
              pattern="[a-z][a-z0-9\-]{0,47}"
              placeholder="my-api"
            />,
          )}
          {field('Git ref', <input class="input" name="git_ref" value="HEAD" />)}
          {field(
            'HTTPS Git URL',
            <input
              class="input"
              name="repository"
              required
              type="url"
              placeholder="https://github.com/your-org/your-api.git"
            />,
            true,
          )}
          {field(
            'Absolute host share directory',
            <input class="input" name="mounts" required placeholder="/Users/you/defuss/my-api" />,
            true,
          )}
          {field(
            'Repository subdirectory',
            <input class="input" name="subdirectory" placeholder="Root by default" />,
          )}
          {field('Health path', <input class="input" name="health" value="/health" />)}
          {field(
            'Host port',
            <input
              class="input"
              name="host_port"
              type="number"
              min="1024"
              max="65535"
              value="8080"
              required
            />,
          )}
          {field(
            'Guest port',
            <input
              class="input"
              name="guest_port"
              type="number"
              min="1"
              max="65535"
              value="3000"
              required
            />,
          )}
          {field(
            'vCPUs',
            <input class="input" name="cpus" type="number" min="1" max="64" value="2" required />,
          )}
          {field(
            'Memory / MiB',
            <input
              class="input"
              name="memory"
              type="number"
              min="256"
              max="262144"
              value="1024"
              required
            />,
          )}
          {field(
            'Environment / JSON',
            <textarea class="textarea" name="env" rows={3}>
              {'{"PORT":"3000"}'}
            </textarea>,
            true,
          )}
          <label class="label checkbox wide">
            <input name="build" type="checkbox" /> Run package build script
          </label>
        </div>
        <p class="muted">
          Published ports use the engine’s host listener. Apply host firewall rules if access must
          be limited.
        </p>
        <pre id="form-error" class="inline-error"></pre>
        <div class="dialog-footer">
          <button
            class="btn"
            data-variant="outline"
            type="button"
            onClick={() => dialog('create').close()}
          >
            Cancel
          </button>
          <button class="btn" type="submit">
            Create application
          </button>
        </div>
      </form>
    </dialog>
  );
}

// The theme follows the OS: defuss-shadcn switches on `class="dark"` on <html>, and nothing sets it
// by itself.
const dark = matchMedia('(prefers-color-scheme: dark)');
const theme = () => document.documentElement.classList.toggle('dark', dark.matches);
theme();
dark.addEventListener('change', theme);

// Start-up: mount the shell once, hide Reconnect until the first failed poll, then poll. The grids
// initialize on their own (MutationObserver), so the first poll needs no wait for them.
df$('#app').morph(<Shell />);
$('reconnect').hidden = true;
renderApp();
void refresh();
