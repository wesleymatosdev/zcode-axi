// fleet-approval-delegate — per-worker tmux pane watcher that clears
// pre-approved permission dialogs and escalates everything else.
//
// The stall class this kills: a fleet TUI worker pauses on a Claude-Code-
// shaped permission dialog ("Do you want to proceed?" + numbered options);
// nothing terminal-states, watchers time out, and the status file's last
// line still looks like cruising. The delegate polls the pane (cheap
// capture-pane) and:
//
//   - allowlisted dialog (fleet/approval-policy.txt) → presses the best-
//     scoped numbered option, audited to logs/<task-id>.approvals.log as
//     `approval:auto class=<name> <prompt-head>`;
//   - everything else → answers nothing, appends `needs-decision: pending
//     approval dialog: <prompt-head>` to the status file ONCE per distinct
//     dialog, and publishes one fleet-event so Telegram surfaces it once;
//   - non-prompt output → never touched (keys are only ever sent for a
//     matched dialog).
//
// Usage:
//
//	fleet-approval-delegate [flags] <task-id>        watch until the window
//	                                                 dies or the task closes
//	fleet-approval-delegate --once [flags] <task-id> one bounded clear (the
//	                                                 fleet-wait revival path)
//	fleet-approval-delegate classify <task-id>       print one classification
//	                                                 line for fleet-wait's
//	                                                 revival path:
//	                                                 approval-dialog <head> |
//	                                                 dead-pane <evidence> |
//	                                                 alive-recent <age-s> |
//	                                                 alive-silent <age-s>
//
// Flags: --interval SEC (watch poll, default 5), --policy FILE, --session
// NAME (default $FLEET_TMUX_SESSION, else zswarm), --status FILE, --log
// FILE (approvals audit), --event-cmd CMD (default <fleet-root>/fleet-event).
//
// Exit: 0 when the window is gone or the task reached done:/failed: (the
// archive sweep kills the window; the delegate must not outlive its worker).
// No external dependencies beyond the Go stdlib and the tmux CLI.
package main

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"syscall"
	"time"
)

const defaultPolicy = `# Compiled-in fallback — kept identical to fleet/approval-policy.txt.
# The file on disk wins when present. Matching is two-pass: deny wins over
# allow regardless of order; unmatched escalates.
allow read-own-tree Read file.*Read\(({{HOME}}/projects/personal/|{{CWD}}/)
allow dir-access Run shell command.*({{HOME}}/projects/personal/|{{CWD}}/)
allow file-edit make this edit to ({{CWD}}/|{{STATUS}}|{{STATUS_BASE}})
allow git-readonly Run shell command.*git (status|log|diff|show|branch|rev-parse|remote -v|describe|ls-files)
allow gh-readonly Run shell command.*gh (pr view|pr checks|pr list|pr diff|issue view|issue list|issue status|repo view|run view|run list|release view)
allow npm-readonly Run shell command.*npm (list|ls|view|outdated|search)
allow status-write (Edit|Write|Update) file.*{{STATUS}}
allow trust-folder [Dd]o you trust (the files in )?this folder
deny network-mutating Run shell command.*(git push|git remote (add|remove|set-url|prune|update)|npm (install|ci|publish|add|remove|unlink)|yarn (add|remove|publish)|pnpm (add|install|publish)|pip (install|publish)|cargo (install|publish)|brew (install|upgrade|uninstall)|deploy|flyctl|vercel|netlify|wrangler|gcloud|aws)
deny raw-fetch Run shell command.*(curl|wget)
deny delete Run shell command.*\brm\b
deny destructive-git Run shell command.*git (reset --hard|clean|checkout --|restore|filter-branch|rebase|merge)
deny gh-mutating Run shell command.*gh (pr (create|merge|close|edit)|issue (create|close|edit)|repo (create|fork|delete)|release (create|delete)|auth)
deny auth-shaped (?i)(login|logout|credential|api[ _-]?key|token)
deny install-cmd (?i)Run shell command.*(apt|apt-get|brew install|gem install|cargo add)

# ---- current ZCode TUI shape (zcode-app-cli 3.11.x) -------------------------
# The dialog header is "Permission · <Tool>" (e.g. "Permission · Bash") and
# the command/path lines sit BELOW the header; options are arrow-selected
# ("→ Allow once" preselected) and confirmed with Enter. Same classes, new
# prompt anchor — deny still wins.
allow read-own-tree Permission · Read.*({{HOME}}/projects/personal/|{{CWD}}/)
allow dir-access Permission · (Bash|Shell).*({{HOME}}/projects/personal/|{{CWD}}/)
allow file-edit Permission · (Edit|Write|Update).*({{CWD}}/|{{STATUS}})
allow git-readonly Permission · (Bash|Shell).*git (status|log|diff|show|branch|rev-parse|remote -v|describe|ls-files)
allow gh-readonly Permission · (Bash|Shell).*gh (pr view|pr checks|pr list|pr diff|issue view|issue list|issue status|repo view|run view|run list|release view)
allow npm-readonly Permission · (Bash|Shell).*npm (list|ls|view|outdated|search)
allow status-write Permission · (Edit|Write|Update).*{{STATUS}}
deny network-mutating Permission · (Bash|Shell).*(git push|git remote (add|remove|set-url|prune|update)|git (reset --hard|clean|filter-branch)|npm (install|ci|publish|add|remove|unlink)|yarn (add|remove|publish)|pnpm (add|install|publish)|pip (install|publish)|cargo (install|publish)|brew (install|upgrade|uninstall)|deploy|flyctl|vercel|netlify|wrangler|gcloud|aws)
deny raw-fetch Permission · (Bash|Shell).*(curl|wget)
deny delete Permission · (Bash|Shell).*\brm\b
deny destructive-git Permission · (Bash|Shell).*git (reset --hard|clean|checkout --|restore|filter-branch|rebase|merge)
deny gh-mutating Permission · (Bash|Shell).*gh (pr (create|merge|close|edit)|issue (create|close|edit)|repo (create|fork|delete)|release (create|delete)|auth)
deny install-cmd (?i)Permission · (Bash|Shell).*(apt|apt-get|brew install|gem install|cargo add)
`

type rule struct {
	action string
	name   string
	re     *regexp.Regexp
}

type paneOption struct {
	num  string
	text string
}

type dialog struct {
	head       string // first line of the prompt (for status/event text)
	promptText string // region text EXCLUDING option lines (for policy matching)
	text       string // normalized region text (kept for debugging)
	options    []paneOption
	arrow      bool   // current ZCode arrow UI (Enter activates the → option)
	fp         string // fingerprint of the dialog (head + options)
}

type handledState struct {
	count     int
	lastActed time.Time
	escalated bool
}

type delegate struct {
	taskID     string
	session    string
	fleetRoot  string
	statusPath string
	auditPath  string
	eventCmd   string
	policyFile string
	interval   time.Duration
	once       bool
	classify   bool
	policy     []rule
	taskCwd    string
	handled    map[string]*handledState
}

var (
	optRe = regexp.MustCompile(`^\s*[❯>✓*]?\s*(\d+)[.)]\s+(\S.*)$`)
	// Arrow-selected option (current ZCode TUI): "→ Allow once   Approve
	// only this request" — the label is the first column; the description
	// column (2+ spaces away) is stripped.
	arrowOptRe     = regexp.MustCompile(`^\s*[→❯>]\s*(\S.*?)(?:\s{2,}.*)?$`)
	unlabeledOptRe = regexp.MustCompile(`^\s*(\S.*?)(?:\s{2,}.*)?$`)
	// Dialog triggers: the proceed shape (read/bash), the file-edit shape
	// ("Do you want to make this edit to <path>?"), the folder-trust
	// prompt at TUI startup, and the current ZCode permission header
	// ("Permission · <Tool>", e.g. "Permission · Bash").
	triggerRe    = regexp.MustCompile(`Do you want to (proceed|make this edit|run)`)
	newTriggerRe = regexp.MustCompile(`Permission ·`)
	trustRe      = regexp.MustCompile(`(?i)do you trust (the files in )?this folder`)
	footerRe     = regexp.MustCompile(`(?i)(esc to cancel|esc cancel|tab to amend|enter to confirm|press enter|type to filter|up/down|ctrl\+o|pgup|pgdn)`)
	// Pre-option hint line ("Filter: type to search") — skipped like a
	// footer but does NOT end the option list (the real nav footer does).
	hintRe = regexp.MustCompile(`(?i)(filter:|type to search)`)
	// Risk banner lines of the current ZCode shape — context, never the
	// command, and never policy-relevant.
	bannerRe  = regexp.MustCompile(`^\s*(High risk tools? require[sd]? explicit approval|Risk: \w+)`)
	spawnedRe = regexp.MustCompile(`^spawned: .* cwd=(\S+) pid=`)
	// Option preference for allow answers: most session-scoped first.
	// "switch to accept edits" converges a manual-mode session toward the
	// fleet's default (yolo) with one keypress.
	scopedOptRe = regexp.MustCompile(`(?i)(always allow access|allow reading from|don['’‘]t ask|accept edits)`)
	yesOptRe    = regexp.MustCompile(`(?i)^yes\b`)
	// The arrow UI's preselected option may only be activated when it is
	// an affirmative ("Allow once") — never navigate or blind-Enter.
	allowOnceRe = regexp.MustCompile(`(?i)^allow\b`)
)

// Knobs the tests override via env; the defaults are the production shape.
// How often the same dialog fingerprint may be auto-answered before we
// conclude the keys are not landing and hand it to a human, etc.
var (
	maxAnswerPerDialog = envInt("FLEET_DELEGATE_MAX_ANSWERS", 3)
	answerCooldown     = time.Duration(envInt("FLEET_DELEGATE_ANSWER_COOLDOWN_SEC", 60)) * time.Second
	activityFreshSecs  = envInt("FLEET_DELEGATE_ACTIVITY_FRESH_SEC", 300)
	stableGap          = time.Duration(envInt("FLEET_DELEGATE_STABLE_GAP_MS", 400)) * time.Millisecond
)

func envInt(name string, def int) int {
	if v := os.Getenv(name); v != "" {
		if n, err := strconv.Atoi(v); err == nil && n >= 0 {
			return n
		}
	}
	return def
}

func main() {
	d, err := parseArgs(os.Args[1:])
	if err != nil {
		fmt.Fprintf(os.Stderr, "fleet-approval-delegate: %v\n", err)
		usage()
		os.Exit(1)
	}
	switch {
	case d.classify:
		classify(d)
	case d.once:
		runOnce(d)
	default:
		watch(d)
	}
}

func usage() {
	fmt.Fprintln(os.Stderr, "usage: fleet-approval-delegate [--interval SEC] [--once] [--policy FILE] [--session NAME] [--status FILE] [--log FILE] [--event-cmd CMD] <task-id>")
	fmt.Fprintln(os.Stderr, "       fleet-approval-delegate classify <task-id>")
}

// parseArgs is hand-rolled to mirror the fleet scripts' space-separated
// flag style; unknown flags are a hard error (never silently ignored).
func parseArgs(argv []string) (*delegate, error) {
	d := &delegate{
		fleetRoot: os.Getenv("FLEET_ROOT"),
		session:   os.Getenv("FLEET_TMUX_SESSION"),
		interval:  5 * time.Second,
		handled:   map[string]*handledState{},
	}
	if d.session == "" {
		d.session = "zswarm"
	}
	if d.fleetRoot == "" {
		if exe, err := os.Executable(); err == nil {
			d.fleetRoot = filepath.Dir(exe)
		} else {
			d.fleetRoot = "."
		}
	}

	var args []string
	for i := 0; i < len(argv); i++ {
		a := argv[i]
		val := func(what string) string {
			if i+1 >= len(argv) {
				fmt.Fprintf(os.Stderr, "fleet-approval-delegate: %s requires a value\n", what)
				os.Exit(1)
			}
			i++
			return argv[i]
		}
		switch {
		case a == "classify":
			d.classify = true
		case a == "--once":
			d.once = true
		case a == "--interval":
			sec, err := strconv.ParseFloat(val("--interval"), 64)
			if err != nil || sec <= 0 {
				return nil, fmt.Errorf("--interval wants positive seconds")
			}
			d.interval = time.Duration(sec * float64(time.Second))
		case a == "--policy":
			d.policyFile = val("--policy")
		case a == "--session":
			d.session = val("--session")
		case a == "--status":
			d.statusPath = val("--status")
		case a == "--log":
			d.auditPath = val("--log")
		case a == "--event-cmd":
			d.eventCmd = val("--event-cmd")
		case strings.HasPrefix(a, "--"):
			return nil, fmt.Errorf("unknown flag: %s", a)
		default:
			args = append(args, a)
		}
	}
	if len(args) != 1 {
		return nil, fmt.Errorf("exactly one <task-id> is required")
	}
	d.taskID = args[0]
	if d.statusPath == "" {
		d.statusPath = filepath.Join(d.fleetRoot, "state", d.taskID+".status")
	}
	if d.auditPath == "" {
		d.auditPath = filepath.Join(d.fleetRoot, "logs", d.taskID+".approvals.log")
	}
	if d.eventCmd == "" {
		d.eventCmd = filepath.Join(d.fleetRoot, "fleet-event")
	}
	if d.policyFile == "" {
		d.policyFile = filepath.Join(d.fleetRoot, "approval-policy.txt")
	}
	d.taskCwd = parseTaskCwd(d.statusPath)
	d.policy = loadPolicy(d.policyFile, d.taskCwd, d.statusPath)
	return d, nil
}

// parseTaskCwd reads the task's cwd from its spawned: line; empty when the
// file is missing or the delegate attached before the line landed (policy
// rules referencing {{CWD}} then simply never match — fail-safe).
func parseTaskCwd(statusPath string) string {
	b, err := os.ReadFile(statusPath)
	if err != nil {
		return ""
	}
	cwd := ""
	for _, ln := range strings.Split(string(b), "\n") {
		if m := spawnedRe.FindStringSubmatch(ln); m != nil {
			cwd = m[1]
		}
	}
	return strings.TrimRight(cwd, "/")
}

func loadPolicy(path, cwd, statusPath string) []rule {
	data, err := os.ReadFile(path)
	if err != nil {
		data = []byte(defaultPolicy)
	}
	var rules []rule
	for _, ln := range strings.Split(string(data), "\n") {
		ln = strings.TrimSpace(ln)
		if ln == "" || strings.HasPrefix(ln, "#") {
			continue
		}
		fields := strings.Fields(ln)
		if len(fields) < 3 || (fields[0] != "allow" && fields[0] != "deny") {
			continue
		}
		expr := expandPlaceholders(strings.Join(fields[2:], " "), cwd, statusPath)
		re, err := regexp.Compile(expr)
		if err != nil {
			fmt.Fprintf(os.Stderr, "fleet-approval-delegate: bad policy regex (%s %s): %v\n", fields[0], fields[1], err)
			continue
		}
		rules = append(rules, rule{action: fields[0], name: fields[1], re: re})
	}
	return rules
}

func expandPlaceholders(expr, cwd, statusPath string) string {
	home, _ := os.UserHomeDir()
	expr = strings.ReplaceAll(expr, "{{HOME}}", regexp.QuoteMeta(home))
	if cwd != "" {
		expr = strings.ReplaceAll(expr, "{{CWD}}", regexp.QuoteMeta(cwd))
	} else {
		// Delegate attached before the spawned: line — an empty {{CWD}}
		// alternative would match every path; make the rule inert until
		// watch() re-loads the policy with the real cwd.
		expr = strings.ReplaceAll(expr, "{{CWD}}", "\x00")
	}
	expr = strings.ReplaceAll(expr, "{{STATUS}}", regexp.QuoteMeta(statusPath))
	// Edit dialogs often show only the file's basename.
	expr = strings.ReplaceAll(expr, "{{STATUS_BASE}}", regexp.QuoteMeta(filepath.Base(statusPath)))
	return expr
}

// ---- tmux interaction -------------------------------------------------------

func (d *delegate) window() string { return fmt.Sprintf("%s:fleet-%s", d.session, d.taskID) }

func windowExists(session, taskID string) bool {
	out, err := exec.Command("tmux", "list-windows", "-t", session, "-F", "#{window_name}").Output()
	if err != nil {
		return false
	}
	want := "fleet-" + taskID
	for _, ln := range strings.Split(string(out), "\n") {
		if strings.TrimSpace(ln) == want {
			return true
		}
	}
	return false
}

func capturePane(window string) (string, bool) {
	out, err := exec.Command("tmux", "capture-pane", "-p", "-J", "-S", "-60", "-t", window).Output()
	if err != nil {
		// Older tmux without -J: retry plain; the option-continuation
		// joining in detectDialog still handles wrapped options.
		out, err = exec.Command("tmux", "capture-pane", "-p", "-S", "-60", "-t", window).Output()
		if err != nil {
			return "", false
		}
	}
	return string(out), true
}

func paneDead(session, taskID string) bool {
	out, err := exec.Command("tmux", "list-panes", "-t", fmt.Sprintf("%s:fleet-%s", session, taskID), "-F", "#{pane_dead}").Output()
	if err != nil {
		return false // window-missing is reported separately
	}
	return strings.TrimSpace(strings.Split(string(out), "\n")[0]) == "1"
}

// detectDialog finds the first permission dialog in the pane. Recognized
// shapes: the proceed shape ("Do you want to proceed?" with numbered
// options), the file-edit shape, the folder-trust prompt, and the current
// ZCode permission header ("Permission · <Tool>" with arrow-selected
// options). Only a matched trigger line counts; anything else returns nil
// and the caller never touches the pane.
func detectDialog(pane string) *dialog {
	lines := strings.Split(pane, "\n")
	for i, ln := range lines {
		arrow := newTriggerRe.MatchString(ln)
		if !arrow && !triggerRe.MatchString(ln) && !trustRe.MatchString(ln) {
			continue
		}
		start := i - 8
		if start < 0 {
			start = 0
		}
		end := i + 15
		if end > len(lines) {
			end = len(lines)
		}
		dg := &dialog{arrow: arrow}
		// Prompt head: nearest non-empty line above the trigger that is not
		// itself an option/footer line — usually the command or file path.
		for j := i - 1; j >= start; j-- {
			t := strings.TrimSpace(lines[j])
			if t == "" || optRe.MatchString(lines[j]) || footerRe.MatchString(t) {
				continue
			}
			dg.head = t
			break
		}
		var promptBelow []string
		if arrow {
			// Current ZCode shape: the command lives BELOW the header,
			// preceded by risk-banner lines and followed by the "Filter:"
			// hint, arrow options, and the nav footer. An arrow trigger
			// with no option line at all is prose, not a dialog — skip.
			seenFooter := false
			for j := i + 1; j < end; j++ {
				t := strings.TrimSpace(lines[j])
				if hintRe.MatchString(t) {
					continue
				}
				if footerRe.MatchString(t) {
					seenFooter = true
					continue
				}
				if bannerRe.MatchString(lines[j]) || t == "" {
					continue
				}
				if m := optRe.FindStringSubmatch(lines[j]); m != nil {
					dg.options = append(dg.options, paneOption{num: m[1], text: strings.TrimSpace(m[2])})
					continue
				}
				if m := arrowOptRe.FindStringSubmatch(lines[j]); m != nil {
					dg.options = append(dg.options, paneOption{num: "→", text: strings.TrimSpace(m[1])})
					continue
				}
				if len(dg.options) == 0 {
					promptBelow = append(promptBelow, t)
					continue
				}
				if seenFooter || len(t) <= 2 || triggerRe.MatchString(lines[j]) || newTriggerRe.MatchString(lines[j]) {
					continue
				}
				if m := unlabeledOptRe.FindStringSubmatch(t); m != nil {
					dg.options = append(dg.options, paneOption{text: strings.TrimSpace(m[1])})
				}
			}
			if len(dg.options) == 0 {
				continue
			}
			if len(promptBelow) > 0 {
				dg.head = promptBelow[0]
			} else {
				dg.head = strings.TrimSpace(ln)
			}
		} else if dg.head == "" {
			dg.head = strings.TrimSpace(ln)
		}
		// Numbered options below the trigger; wrapped continuation lines
		// fold into the previous option (capture-pane -J joins most wraps).
		// Guards so stray keystrokes can't perturb the dialog identity:
		// 1–2 char lines (e.g. a TTY-echoed answer digit) never fold, and
		// nothing folds past the footer — a real dialog ends there.
		if !arrow {
			seenFooter := false
			for _, rl := range lines[i+1 : end] {
				if m := optRe.FindStringSubmatch(rl); m != nil {
					dg.options = append(dg.options, paneOption{num: m[1], text: strings.TrimSpace(m[2])})
					continue
				}
				t := strings.TrimSpace(rl)
				if footerRe.MatchString(t) {
					seenFooter = true
					continue
				}
				if t == "" || seenFooter || len(t) <= 2 || triggerRe.MatchString(rl) {
					continue
				}
				if n := len(dg.options); n > 0 {
					dg.options[n-1].text = strings.TrimSpace(dg.options[n-1].text + " " + t)
				}
			}
		}
		// Prompt text for policy matching: the few lines above and
		// including the trigger line (the tool header + command), plus —
		// in the current ZCode shape — the command lines below the
		// header. Option lines are deliberately EXCLUDED (both the
		// numbered labels and the arrow UI's label+description columns) —
		// a "git push" dialog must classify on the command, not on the
		// option wording. The window is small so stale scrollback above
		// the dialog can't leak into classification.
		promptStart := i - 5
		if promptStart < start {
			promptStart = start
		}
		var pb strings.Builder
		for _, rl := range lines[promptStart : i+1] {
			t := strings.TrimSpace(rl)
			if t == "" {
				continue
			}
			if pb.Len() > 0 {
				pb.WriteByte(' ')
			}
			pb.WriteString(t)
		}
		for _, t := range promptBelow {
			pb.WriteByte(' ')
			pb.WriteString(t)
		}
		dg.promptText = pb.String()
		// Full normalized region (debugging aid, not matched against).
		var b strings.Builder
		for _, rl := range lines[start:end] {
			t := strings.TrimSpace(rl)
			if t == "" {
				continue
			}
			if b.Len() > 0 {
				b.WriteByte(' ')
			}
			b.WriteString(t)
		}
		dg.text = b.String()
		// Fingerprint: head + options — stable under pane redraw/scroll
		// (the surrounding region text shifts as the pane repaints).
		var fb strings.Builder
		fb.WriteString(dg.head)
		for _, o := range dg.options {
			fb.WriteString("\x1f" + o.num + "\x1f" + o.text)
		}
		sum := sha256.Sum256([]byte(fb.String()))
		dg.fp = hex.EncodeToString(sum[:])[:16]
		return dg
	}
	return nil
}

// ---- decision + action ------------------------------------------------------

// decide applies policy rules against the PROMPT text (never the option
// wording) in two passes: any deny match escalates, then allow rules are
// tried in file order; no match is an escalate (default-deny). Deny always
// wins regardless of file order.
func (d *delegate) decide(dg *dialog) (string, string) {
	for _, r := range d.policy {
		if r.action == "deny" && r.re.MatchString(dg.promptText) {
			return "deny", r.name
		}
	}
	for _, r := range d.policy {
		if r.action == "allow" && r.re.MatchString(dg.promptText) {
			return "allow", r.name
		}
	}
	return "deny", "unmatched"
}

// pickOption chooses which key to press for an allow verdict. Arrow UI: the
// preselected (→) option is what Enter activates — only press Enter when
// that option is an affirmative ("Allow once"); never navigate blind, and
// return "" (escalate) when the preselection is anything else. Numbered UI:
// the most session-scoped affirmative available, else plain yes, else the
// first option. A dialog with no activatable key is not safe to answer —
// returns "" and the caller escalates instead.
func pickOption(dg *dialog) string {
	if dg.arrow {
		for _, o := range dg.options {
			if o.num == "→" {
				if allowOnceRe.MatchString(o.text) {
					return "Enter"
				}
				return ""
			}
		}
		return ""
	}
	if len(dg.options) == 0 {
		return ""
	}
	for _, o := range dg.options {
		if scopedOptRe.MatchString(o.text) {
			return o.num
		}
	}
	for _, o := range dg.options {
		if yesOptRe.MatchString(o.text) {
			return o.num
		}
	}
	return dg.options[0].num
}

func (d *delegate) audit(format string, a ...any) {
	f, err := os.OpenFile(d.auditPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return
	}
	defer f.Close()
	fmt.Fprintf(f, "%s ", time.Now().UTC().Format("2006-01-02T15:04:05Z"))
	fmt.Fprintf(f, format+"\n", a...)
}

func (d *delegate) appendStatus(line string) {
	f, err := os.OpenFile(d.statusPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return
	}
	defer f.Close()
	fmt.Fprintf(f, "%s\n", truncate(line, 480))
}

func truncate(s string, n int) string {
	s = strings.Join(strings.Fields(s), " ")
	if len(s) <= n {
		return s
	}
	return s[:n-1] + "…"
}

func (d *delegate) publishEvent(detail string) {
	_ = exec.Command(d.eventCmd, d.taskID, "needs-decision", detail).Run()
}

func (d *delegate) sendKey(key string) bool {
	return exec.Command("tmux", "send-keys", "-t", d.window(), key).Run() == nil
}

// handleDialog acts on a stable dialog: auto-answer for allow, escalate for
// deny. Returns what it did.
func (d *delegate) handleDialog(dg *dialog) string {
	hs := d.handled[dg.fp]
	if hs == nil {
		hs = &handledState{}
		d.handled[dg.fp] = hs
	}
	if hs.count > 0 && time.Since(hs.lastActed) < answerCooldown {
		return "cooldown"
	}
	action, class := d.decide(dg)
	head := truncate(dg.head, 160)
	if action == "allow" {
		opt := pickOption(dg)
		if opt == "" {
			// No activatable key (no numbered options, or the arrow UI's
			// preselected option is not an affirmative) → never blind-answer.
			if !hs.escalated {
				d.escalate(dg, class+" (no activatable option)")
				hs.escalated = true
			}
			return "escalated"
		}
		if hs.count >= maxAnswerPerDialog {
			// Same dialog keeps coming back — keys probably not landing;
			// stop pressing, hand it to a human.
			if !hs.escalated {
				d.escalate(dg, class+" (answer limit reached)")
				hs.escalated = true
			}
			return "escalated"
		}
		if !d.sendKey(opt) {
			d.audit("approval:error fp=%s class=%s keys-failed prompt=%s", dg.fp, class, head)
			return "error"
		}
		hs.count++
		hs.lastActed = time.Now()
		d.audit("approval:auto fp=%s class=%s option=%s prompt=%s", dg.fp, class, opt, head)
		return "answered"
	}
	// deny / unmatched: answer nothing, surface once per distinct dialog.
	if !hs.escalated {
		d.escalate(dg, class)
		hs.escalated = true
	}
	return "escalated"
}

// escalatedBefore checks the audit trail: this exact dialog (fingerprint)
// already escalated — skip the status append and the event so Telegram and
// the status file surface it ONCE, across processes (watch delegate vs the
// fleet-wait --once clear).
func (d *delegate) escalatedBefore(fp string) bool {
	b, err := os.ReadFile(d.auditPath)
	if err != nil {
		return false
	}
	return strings.Contains(string(b), "approval:escalate fp="+fp+" ")
}

func (d *delegate) escalate(dg *dialog, class string) {
	if d.escalatedBefore(dg.fp) {
		return
	}
	head := truncate(dg.head, 160)
	opts := make([]string, 0, len(dg.options))
	for _, o := range dg.options {
		opts = append(opts, truncate(o.text, 80))
	}
	detail := fmt.Sprintf("pending approval dialog: %s [delegate: %s; options: %s]",
		head, class, strings.Join(opts, " | "))
	d.appendStatus(fmt.Sprintf("needs-decision: %s", detail))
	d.audit("approval:escalate fp=%s class=%s prompt=%s", dg.fp, class, head)
	d.publishEvent(detail)
}

// ---- modes ------------------------------------------------------------------

func (d *delegate) poll() {
	pane, ok := capturePane(d.window())
	if !ok {
		return
	}
	if dg := detectDialog(pane); dg != nil {
		d.handleDialog(dg)
	}
}

// runOnce is the bounded clear used by fleet-wait's revival path: capture,
// re-capture after a short gap, and act only if the same dialog is still
// up (never answer a mid-repaint frame). If a long-running watcher delegate
// already owns this task's pane, stand down — two actors sending keys to
// the same TUI risks a stray digit answering the WRONG next dialog.
func runOnce(d *delegate) {
	if watcherRunning(os.Getpid(), d.taskID) {
		return
	}
	pane, ok := capturePane(d.window())
	if !ok {
		return
	}
	dg := detectDialog(pane)
	if dg == nil {
		return
	}
	first := dg.fp
	time.Sleep(stableGap)
	pane, ok = capturePane(d.window())
	if !ok {
		return
	}
	dg = detectDialog(pane)
	if dg != nil && dg.fp == first {
		d.handleDialog(dg)
	}
}

// watcherRunning reports whether another fleet-approval-delegate process is
// watching this task (its command line ends with the task-id).
func watcherRunning(self int, taskID string) bool {
	out, err := exec.Command("pgrep", "-f",
		"fleet-approval-delegate( [^-][^ ]*)*( --[^ ]+ [^ ]+)* "+regexp.QuoteMeta(taskID)+"$").Output()
	if err != nil {
		return false
	}
	for _, f := range strings.Fields(string(out)) {
		if pid, err := strconv.Atoi(f); err == nil && pid != self {
			return true
		}
	}
	return false
}

func watch(d *delegate) {
	sig := make(chan os.Signal, 1)
	signal.Notify(sig, syscall.SIGINT, syscall.SIGTERM)
	stop := make(chan struct{})
	go func() {
		<-sig
		close(stop)
	}()

	// A dialog must be seen on two consecutive polls before we act (never
	// answer a mid-repaint frame). At the default 5s interval that is ≤10s
	// reaction time — cheap insurance against answering a transient pane.
	var lastFP string
	for {
		select {
		case <-stop:
			return
		default:
		}
		if terminalReached(d.statusPath) || !windowExists(d.session, d.taskID) {
			return
		}
		// fleet-spawn attaches the delegate before the worker's spawned:
		// line lands, so the task cwd ({{CWD}} policy rules) may be unknown
		// at startup — re-resolve until it appears, then load the policy
		// once more with the real cwd.
		if d.taskCwd == "" {
			if cwd := parseTaskCwd(d.statusPath); cwd != "" {
				d.taskCwd = cwd
				d.policy = loadPolicy(d.policyFile, d.taskCwd, d.statusPath)
			}
		}
		if pane, ok := capturePane(d.window()); ok {
			if dg := detectDialog(pane); dg != nil {
				if dg.fp == lastFP {
					d.handleDialog(dg)
				}
				lastFP = dg.fp
			} else {
				lastFP = ""
			}
		}
		select {
		case <-stop:
			return
		case <-time.After(d.interval):
		}
	}
}

func terminalReached(statusPath string) bool {
	b, err := os.ReadFile(statusPath)
	if err != nil {
		return false
	}
	s := strings.TrimRight(string(b), "\n")
	if s == "" {
		return false
	}
	lines := strings.Split(s, "\n")
	last := lines[len(lines)-1]
	return strings.HasPrefix(last, "done:") || strings.HasPrefix(last, "failed:")
}

// classify prints one machine-readable line for fleet-wait's revival path.
func classify(d *delegate) {
	if !windowExists(d.session, d.taskID) {
		fmt.Println("dead-pane window-missing")
		return
	}
	if paneDead(d.session, d.taskID) {
		fmt.Println("dead-pane pane-dead")
		return
	}
	pane, ok := capturePane(d.window())
	if !ok {
		fmt.Println("dead-pane capture-failed")
		return
	}
	if dg := detectDialog(pane); dg != nil {
		fmt.Printf("approval-dialog %s\n", truncate(dg.head, 160))
		return
	}
	age := logAgeSeconds(filepath.Join(d.fleetRoot, "logs", d.taskID+".log"))
	if age < 0 {
		fmt.Println("alive-silent no-log")
		return
	}
	if age < activityFreshSecs {
		fmt.Printf("alive-recent %d\n", age)
		return
	}
	fmt.Printf("alive-silent %d\n", age)
}

func logAgeSeconds(path string) int {
	st, err := os.Stat(path)
	if err != nil {
		return -1
	}
	return int(time.Since(st.ModTime()).Seconds())
}
