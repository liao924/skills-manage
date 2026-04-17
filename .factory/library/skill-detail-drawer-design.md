# Skill Detail Drawer — Design Contract (Milestone 4)

Worker-facing reference for the Universal Skill Detail Drawer. Describes the shared `SkillDetailView` plus two outer shells (`SkillDetailPage` for `/skill/:skillId`, `SkillDetailDrawer` for list-entry surfaces).

Do NOT split logic across two parallel components. Drawer and route full-page must render the same `SkillDetailView` internally.

## Component tree

```
SkillDetailPage (route `/skill/:skillId`)
  └── PageHeader  (back + breadcrumb from location.state.from)
  └── SkillDetailView variant="page" leading={null}

SkillDetailDrawer (portal, list-entry surfaces)
  └── Overlay  (bg-black/30, click → close)
  └── DrawerShell  width: desktop min(900px, 90vw), <768px 100vw
      └── DrawerTopBar  h-10 shrink-0, just a × close button (no title)
      └── SkillDetailView variant="drawer" leading={null} onRequestClose={...}

SkillDetailView (shared core)
  ├── ViewHeader
  │     ├── {leading?} slot (currently always null from both shells)
  │     ├── TitleBlock  h1 title + p description (truncate)
  │     └── TabToggle  [Markdown | Raw | AI Explanation]
  └── ContentArea
        ├── LoadingState | ErrorState | BrowserFallbackState
        └── TwoColumnLayout
              ├── LeftPreview (tabpanel; flex-1 overflow-auto)
              │     ├── MarkdownPanel
              │     ├── RawPanel
              │     └── ExplanationPanel (header + error + body + empty)
              └── RightSidebar (w-64; at <768px moves below main content)
                    ├── MetadataSection
                    ├── InstallStatusSection (lobster row + coding row, PlatformToggleIcon)
                    └── CollectionsSection (tags + add trigger)
        └── CollectionPickerDialog (portal)
```

## `SkillDetailView` prop contract

```ts
interface SkillDetailViewProps {
  skillId: string;
  variant: "page" | "drawer";         // affects local styling only, never behavior
  leading?: React.ReactNode;          // ViewHeader leftmost slot; currently null from both shells
  onRequestClose?: () => void;        // drawer-only: used to dispatch Esc/×
  scrollContainerRef?: React.Ref<HTMLDivElement>; // optional; allows shell to observe scroll
}
```

Responsibilities inside the View:
- On mount / `skillId` change: `skillDetailStore.loadDetail(skillId)` + `loadCachedExplanation(skillId, lang)`.
- On unmount: `skillDetailStore.reset()` (this runs regardless of shell; `reset()` must be safe).
- Owns local UI state: `activeTab` (markdown|raw|explanation), `isCollectionPickerOpen`, `showErrorDetails`.
- Does NOT render a back button, close button, or breadcrumb. Those belong to the outer shell.

The View must NOT call `useNavigate`, `useParams`, or drawer-specific APIs. All route/shell concerns are handled outside it.

## `SkillDetailPage` (route `/skill/:skillId`)

- `useParams` → `skillId`.
- `useLocation().state?.from?: { pageLabel: string; route: string }` → powers breadcrumb.
- Renders:
  - `PageHeader`: `[← Back]` + breadcrumb. Breadcrumb shape:
    - With `from`: `{pageLabel} › {detail.name || skillId}` (link first crumb to `route`).
    - Without `from`: `{detail.name || skillId}` single segment.
  - `<SkillDetailView skillId={skillId} variant="page" leading={null} />`.
- Full-page exists primarily for direct URL / deep-link / sharing.

## `SkillDetailDrawer` (list-entry surfaces)

```ts
interface SkillDetailDrawerProps {
  open: boolean;
  skillId: string | null;
  onOpenChange: (open: boolean) => void;
  returnFocusRef?: React.Ref<HTMLElement>;
}
```

Behavior:
- Renders into a portal.
- Width: `min(900px, 90vw)` at ≥ 768px. At < 768px: `100vw`.
- At < 768px the inner `RightSidebar` moves below the `LeftPreview` (stack instead of side-by-side). Use a responsive class (e.g. `flex-col lg:flex-row`) on the `TwoColumnLayout` container to achieve this; sidebar becomes full-width with `border-t` rather than `border-l`.
- Esc, overlay click, or close button → `onOpenChange(false)`.
- When `open && skillId`, internally renders `<SkillDetailView skillId={skillId} variant="drawer" leading={null} onRequestClose={() => onOpenChange(false)} />`.
- On close:
  - `skillDetailStore.reset()` fires via the View's unmount cleanup.
  - Focus returns to `returnFocusRef.current` (if provided), otherwise to `document.body` fallback.
- Must NOT remount the underlying list. Integrate via a Zustand slice or local state in the list page.

Accessibility:
- Focus trap while open (use Radix `Dialog` or a vetted primitive; do not hand-roll trap).
- `role="dialog"`, `aria-modal="true"`, `aria-labelledby` pointing to the ViewHeader `h1` id.
- Esc and overlay click are both exit paths.

## AI explanation behavior inside drawer

- Continues using `skillDetailStore` (single source of truth). Do not duplicate state.
- Monotonic request-id logic from M1 remains unchanged.
- `reset()` on drawer close aborts/voids the current in-flight request; reopening the drawer for the same skill must start from cached cached explanation (if any) or clean state, never from stale partial content.
- Friendly error message + expandable raw details must render inside drawer identically to full page.

## Install/uninstall from drawer

- Use the same `skillDetailStore.installSkill` / `uninstallSkill` paths as full page.
- After success, call `Promise.all([platformStore.refreshCounts(), skillDetailStore.refreshInstallations(skillId)])`.
- Underlying list cards re-read from their own Zustand stores and update automatically — do NOT pass callbacks through the drawer.

## Integration points (list entries)

| Surface | Current behavior | M4 behavior |
|---|---|---|
| Central (`/central`) | `navigate('/skill/:id', { state: { scrollRestoration } })` | Open `SkillDetailDrawer`. Do NOT call `saveReturnContext` etc. |
| Platform (`/platform/:agentId`) | Same | Same |
| Collection (`/collections`, `/collection/:id` if any) | `navigate('/skill/:id', { state: { collectionId, scroll } })` + `saveReturnContext` | Open drawer. Active collection in Zustand store remains unchanged. |
| Discover (`/discover/:projectPath`) | `navigate('/skill/:id', { state: { projectPath } })` | Open drawer. Selected project in `discoverStore` remains unchanged. |
| Marketplace | `SkillPreviewDialog` opens | Try to unify into `SkillDetailDrawer`. If diff is too large, keep existing drawer and create follow-up feature; MUST NOT regress current Marketplace drawer behavior. |

After M4, `saveReturnContext` / `consumeReturnContext` / `saveScrollPosition` / `restoreScrollPosition` helpers in `src/lib/scrollRestoration.ts` are not invoked from the four list surfaces above. The helpers themselves remain in the file (no deletions) so the codebase stays safe for future direct-URL navigation flows.

## State-of-the-world guarantees (for validators)

When a user opens a drawer from Central and closes it:
1. URL is unchanged (still `/central`, not `/skill/:id`).
2. Scroll offset on Central list is identical pre/post.
3. Active search input value is unchanged.
4. Previously-focused card regains focus (`returnFocusRef`).
5. Drawer content is not visible in DOM afterward.
6. `skillDetailStore.detail` is `null` (reset) OR is the next skill's detail if the user immediately opened another.
7. Sidebar counts reflect any install/uninstall action performed inside the drawer.

These same guarantees apply to Platform, Collection, Discover (with the respective "selected context" — collection id / project path — also preserved).

## Out of scope for M4

- URL sync for drawer state (e.g. `?skill=xxx`). User explicitly declined.
- Deleting scrollRestoration helpers. Keep them for fallback.
- Migrating deep-link full-page to also use drawer. Full-page remains a distinct surface.
- Changing breadcrumb semantics beyond `location.state.from` resolution.
