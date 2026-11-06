"""Playing a level on what Evil knows: the objectives open to her now, and an explorer that
picks one, plans a way there through the known map, and follows it.

Objectives (what Evil will choose between; the explorer is the stand-in that chooses for
now, nearest first):
- the exit, once she has seen it and can get to it;
- keys she has seen;
- switches and doors she has seen and not tried (what they do is learned by trying);
- the edge of the known map: places she can get to next to ones she hasn't seen.
"""
from __future__ import annotations

import math
from dataclasses import dataclass, field, replace

from evildoom.knowledge.model import Knowledge
from evildoom.motor.follower import TICRATE, Controls, Observation, RouteFollower
from evildoom.nav.reach import (MAX_STEP, PLAYER_HEIGHT, HeightStates, Navigator, TriggerLine,
                                _segment_distance, _segments_cross, apply_trigger, can_cross)
from evildoom.nav.walkthrough import Step, _action_for, steps_from_legs
from evildoom.wad.specials import Category, Key, Lock, Trigger, classify_line

PLAN_SECONDS = 1.5
# how much nearer an objective of each kind counts as (keys, switches first; looking again last)
WEIGHT = {"exit": 0.5, "key": 0.6, "switch": 0.8, "door": 0.8, "frontier": 1.0, "room": 1.1,
          "visit": 2.0, "noise": 0.7, "teleporter": 1.2, "recheck": 3.0, "item": 0.6, "hunt": 1.5}
COMPARED = 4                  # objectives routed and compared by the way there, each pick
GIVE_UP_TICS = 20 * TICRATE   # an objective not done by then is dropped for a while...
ROUTE_SPEED = 8.0             # ...or by the time it takes to walk its route at this (units a tic)
ROUTE_SLACK = 15 * TICRATE    # ...and this
THRESHOLD_IN = 8.0            # a threshold's spot: this far over the line
EXIT_MARGIN = 64.0  # the expert's run: no goal this near a walk-over exit line (she'd slide over)
JUMP_MISSES = 2  # a run-jump missed this often isn't tried again; missed once, only when
#                  there's nothing else to do (it's difficult: another way first)
REPRESS_AFTER = 60 * TICRATE  # a repeatable switch whose effect wasn't seen: another go after this
IDLE_RETRY = TICRATE          # with nothing to do, look for something again this often
RETRY_TICS = 30 * TICRATE     # ...and offered again after this
LOOK_TICS = TICRATE           # a turn on the spot, at the edge of the known map
SHUFFLE_TICS = TICRATE        # an item still there on arrival: shuffled over this long
TOUCH = 36.0                  # a pickup's box and hers overlap this near (20 + 16), either way
TURN_RATE = 20.0              # degrees a tic, turning to look at something
AMBIENT_RADIUS = 96.0         # sounds from about the same place...
AMBIENT_REPEATS = 4           # ...heard more often than this are background
NOISE_TICS = 60 * TICRATE     # a sound is worth going to look into for this long
CAUSE_TICS = TICRATE          # a sound this soon after she set off a line she doesn't know...
URGENT_TICS = 6 * TICRATE     # ...is gone to look at first, for this long (a timed door shuts)
GLANCE_GAP = 5 * TICRATE      # at most one look round this often (stairs build in many steps)
GLANCE_AFTER = 8              # tics after a walk-over line goes off unseen, before looking
HEAR_NEARER = 128.0           # a noise is worth walking towards if it gets her this much nearer
ROOM_SPOTS = 6                # reachable spots tried for seeing an unseen wall from
ROOM_LOOK_RANGE = 768.0       # an unseen wall is looked at from a reachable spot this near it
FRONTIER_BACK = 24.0          # a frontier's look-out: this far back from the way into the unseen
COVER_GAP = 96.0              # a room's walked once nowhere in it is further than this from her steps
WADE_LIMIT = 256.0            # how far over a hurting floor a route may go, normally
WADE_HEALTH = 50.0            # ...and not at all below this health
KEEP_REACH = 0.75             # an objective mustn't leave less than this share reachable


def _compass(bearing: float) -> str:
    """'to the NE' and so on."""
    names = ["E", "NE", "N", "NW", "W", "SW", "S", "SE"]
    return "to the " + names[round(bearing / 45) % 8]


@dataclass
class Objective:
    kind: str  # exit, key, switch, door, teleporter, frontier, room, visit, recheck, noise, item, hunt
    goal: tuple[float, float]
    label: str
    trigger: TriggerLine | None = None
    key: Key | None = None
    reveals: frozenset = frozenset()  # frontier: the unseen polys it looks into
    heard: object = None              # noise: the sound
    look_at: tuple | None = None      # room: the wall to face once there
    risky: bool = False               # nothing safer left: one-way drops, nukage are all right
    pad: frozenset = frozenset()      # teleporter: all the lines of its pad (one objective)
    thing: int | None = None          # item, hunt: the thing's id (the engine's)
    urgent: bool = False              # noise: just after she set something off; go now
    worth: float | None = None        # urgent only by a route no longer than this (a better gun
    #                                   is worth so much walking: further, the fight first)

    @property
    def ident(self) -> tuple:
        line = self.pad or (self.trigger.line if self.trigger else None)
        return (self.kind, line, self.key, self.reveals or None, self.risky, self.thing)

    @property
    def priority(self) -> int:
        """Taken up before anything nearer of a lower one: the exit, then the doors and
        switches a key she has opens (what the key was got for)."""
        if self.kind == "exit":
            return 0
        if self.kind in ("door", "switch") and self.trigger.info.lock is not Lock.NONE:
            return 1
        if self.urgent:
            return 1
        return 2


@dataclass
class Explorer:
    nav: Navigator
    knowledge: Knowledge = None
    current: Objective | None = None
    follower: RouteFollower | None = None
    log: list[str] = field(default_factory=list)
    _dropped: dict = field(default_factory=dict)  # objective ident -> tic dropped
    # ...and how often (each time it waits twice as long to be offered again: MAP14's switch
    # that never worked kept the one that would out of the running)
    _drop_count: dict = field(default_factory=dict)
    _since: int = 0
    _exits_seen: int = 0
    _allowed: int = GIVE_UP_TICS  # how long the current objective may take
    _look_until: int | None = None
    _glance_until: int | None = None
    _glanced: int = 0  # fired lines looked round for so far
    _heard_n: int = 0  # sounds dealt with so far
    _last_glance: int = -10**9
    _sector: int | None = None
    _follower_of: object = None  # the follower whose log has been passed on...
    _follower_seen: int = 0      # ...this far
    _glance_at: float | None = None
    _idle_since: int = -10**9  # when it last found nothing to do
    # Other things to do, from elsewhere (the fight: what to pick up, monsters to hunt down):
    # (obs, reach, pos) -> objectives; `wanted` are offered with the rest, `last_resort` only
    # once there's nothing else; `gone(obs, objective)`: it's no longer there to be done.
    wanted: object = None
    last_resort: object = None
    gone: object = None
    dangerous: object = None  # (obs, objective) -> too dangerous to go for unless nothing else
    interrupt: object = None  # (obs, objective) -> something better to do turned up: choose again
    exit_last: bool = False   # the exit only once there's nothing else to do (the expert's 100% run)
    before_exit: object = None  # (obs) -> True: rather the level again than out (asks restart_wanted)
    restart_wanted: bool = False  # (for whoever runs the game: the level again, knowing what she knows)
    _held_since: int | None = None  # something else has had the controls since then
    _urgent_n: int = 0  # sounds checked for being worth dropping everything for
    _reach: set = field(default_factory=set)  # polys she could get to, as of the last look

    def __post_init__(self) -> None:
        if self.knowledge is None:
            self.knowledge = Knowledge(self.nav)
        self._view = None
        self._view_key = None
        self._all_jumps: list | None = None  # (every run-jump the navigator had: some left out once missed)
        self._difficult = False  # (the jumps missed once are in, for want of anything else)
        self._exits_open = True  # (exit_last: closed till nothing else is left)
        self._pads: set[int] | None = None  # (polys beyond walk-over exit lines)

    # --- the known world ----------------------------------------------------------------

    def states(self) -> HeightStates:
        """The heights the planner should take: as last seen where seen, else as at the start
        (those sectors can't be gone into anyway); and where she set something off she knows
        the effect of and hasn't looked since, as it'll have left it (a floor lowered out of
        sight is down: the planner can't fire a one-shot switch again to get it)."""
        k = self.knowledge
        now = k._last.tic if k._last is not None else 0
        out = {s: {tuple(k.seen_heights[s])} if s in k.seen_heights else {h}
               for s, h in enumerate(self.nav.movers.initial)}
        known = {t.line: (t, sectors) for t, sectors in k.known_triggers()}
        for f in k.fired:
            if f.line not in known or f.life != k.lives:  # (this life's: the level's started again)
                continue
            t, sectors = known[f.line]
            effects = t.effects_now(out)
            for s in sectors:
                if not effects.get(s):
                    continue
                # (not seen since the move should have finished: as it'll have left it; seen
                # after, as seen. Stairs rise slowly: seen part way up is not the top)
                h = effects[s][0]
                timing = self.nav.movers.timing(t.info.special, t.info.description, s, h) if t.line >= 0 else None
                done = f.tic + (timing.travel if timing else 0)
                # (what comes back is only as moved till it's due back: MAP23's timed door,
                # opened a minute before, seemed open still and a path squeezed through it)
                back = done + (timing.hold + timing.travel if timing and timing.hold is not None else 0)
                if not t.permanent and timing is not None and timing.hold is not None and now > back:
                    continue
                if k.seen_tic.get(s, -1) < done:
                    hs = {tuple(h) for h in effects[s]}
                    out[s] = hs if t.permanent else out[s] | hs
        # a lift or a timed door she knows comes back to where it rests, wherever she last saw
        # it (E3M3: seen down, the lift seemed to stay down, and couldn't take her up)
        for t, sectors in known.values():
            if not t.permanent:
                for s in sectors:
                    out[s] = out[s] | {tuple(self.nav.movers.initial[s])}
        # a slot narrower than the box is walked over at its sides' floor, however deep she
        # saw it (E4M1's 8-unit slot by the blue key room, seen at its real floor, was a pit)
        # (at each floor the sides can be at: MAP08's moat beside a floor a trap drops and a
        # switch raises back, taken at the bottom only, kept her in once it was back up)
        for tr in self.nav.trenches:
            floors = {self.nav.trench_floor(tr, lambda o: min(h[0] for h in out[o])),
                      self.nav.trench_floor(tr, lambda o: max(h[0] for h in out[o]))}
            out[tr] = {(floor, h[1]) for h in out[tr] for floor in floors}
        return out

    def view(self, fresh: bool = False) -> Navigator:
        """The navigator as far as Evil knows (rebuilt when what she knows changes). `fresh`:
        for the level from its start (what's spent this life works again; exits open)."""
        k = self.knowledge
        # (once-only door lines used this life open nothing again: MAP19's door, opened for good
        # by its own, was shut by a walk-over line that opens it a while; it seemed openable)
        spent_doors = frozenset() if fresh else frozenset(
            f.line for f in k.fired if f.life == k.lives
            and k._special(f.line) is not None and k._special(f.line).trigger is Trigger.D1)
        exits_open = fresh or self._exits_open
        key = (len(k.sectors), len(k.seen_polys), len(k.visited), sum(map(len, k.learned.values())),
               len(k.teleports), spent_doors, exits_open)
        if fresh or self._view is None or key != self._view_key:
            triggers = []
            trenches = self.nav.trenches
            for t, sectors in k.known_triggers():
                if t.line in spent_doors:
                    continue
                # (and the slots beside them: walked at their sides' floor, they never move to
                # be seen to; MAP08's moat beside the floor the trap drops and a switch raises)
                keep = set(sectors) | {s for s in t.effects
                                       if s in trenches and set(trenches[s]) & set(sectors)}
                effects = {s: hs for s, hs in t.effects.items() if s in keep}
                recompute = t.recompute
                if recompute is not None:
                    recompute = (lambda f, keep: lambda st: {s: h for s, h in f(st).items()
                                                             if s in keep})(recompute, keep)
                known = replace(t, effects=effects, recompute=recompute)
                for extra, value in vars(t).items():  # (set after construction: `relevant`)
                    known.__dict__.setdefault(extra, value)
                triggers.append(known)
            keys_at: dict = {}
            for th in k.known_things():
                if th.type in Key._value2member_map_:
                    keys_at.setdefault(Key(th.type), []).append((th.x, th.y))
            polys = k.known_polys()
            if not exits_open:
                polys = polys - self._exit_pads()  # (the expert's run: not out by walking over the exit)
            view = self.nav.restricted_to(polys, set(k.teleports), triggers, keys_at)
            if fresh:
                return view
            self._view, self._view_key = view, key
        return self._view

    # --- objectives ---------------------------------------------------------------------

    def objectives(self, obs: Observation, risky: bool = False) -> list[Objective]:
        """What Evil could go and do now."""
        nav, k, view = self.nav, self.knowledge, self.view()
        pos = (obs.x, obs.y)
        here = nav.mesh.poly_at(*pos)
        if here is None:
            return []
        keys = set(obs.keys or ())
        states = self.states()
        # (what she could get to using what she's learned: the planner works out how)
        actions = [t for t in view.triggers if t.effects]
        reach, states_all, _ = view._saturate(here.index, states, frozenset(keys), {}, actions)
        self._reach = reach
        out: list[Objective] = []
        by_line = {t.line: t for t in nav.triggers if t.line >= 0}
        for li in k.exits() if self._exits_open else ():
            t = by_line.get(li)
            if t is not None and t.activators & reach and t.info.lock.satisfied(keys):
                out.append(Objective("exit", nav._activation_point(t, pos, reach),
                                     f"exit (line {li})", trigger=t))
        for th in k.known_things():
            if th.type in Key._value2member_map_ and Key(th.type) not in keys:
                spots = [(p, pt) for p, pt in view.pickup_spots(th.x, th.y, states) if p in reach]
                if spots:
                    pt = min(spots, key=lambda s: math.dist(s[1], pos))[1]
                    out.append(Objective("key", pt, f"pick up {Key(th.type).name}", key=Key(th.type)))
        for li in k.interactables():
            t = by_line.get(li)
            if t is None or not t.activators & reach or not t.info.lock.satisfied(keys):
                continue
            kind = "door" if t.info.category in (Category.DOOR, Category.LOCKED_DOOR) else "switch"
            out.append(Objective(kind, nav._activation_point(t, pos, reach),
                                 f"try {kind} (line {li})", trigger=t))
        # repeatable switches pressed a while ago whose effect she never saw: worth another go
        # once there's nothing else (E3M4: a door one opened was shut again by another)
        learned_lines = {nav.triggers[i].line for i, sec in k.learned.items() if sec}
        last_press: dict[int, int] = {f.line: f.tic for f in k.fired}
        again = []
        for li, tic in last_press.items():
            t = by_line.get(li)
            if t is None or li not in k.mapped or li in learned_lines or t.info.trigger not in (Trigger.SR, Trigger.GR):
                continue
            if obs.tic - tic < REPRESS_AFTER or not t.activators & reach or not t.info.lock.satisfied(keys):
                continue
            again.append(Objective("switch", nav._activation_point(t, pos, reach),
                                   f"try switch again (line {li})", trigger=t))
        out += self._teleporters(reach, pos)
        out += self._frontier(reach, pos)
        out += self._recheck(reach, pos)
        out += self._unfinished(reach, pos)
        out += self._unvisited(reach, pos)
        out += self._noises(reach, pos, obs.tic)
        if self.wanted is not None:
            out += self.wanted(obs, reach, pos)
        tic = obs.tic
        def fresh(o: Objective) -> bool:  # (not dropped lately: the longer, the more often it has been)
            times = self._drop_count.get(o.ident, 0)
            return tic - self._dropped.get(o.ident, -10**9) > RETRY_TICS * 2 ** max(0, min(times - 1, 5))
        # (not anywhere that would cut her off from most of what she can get to now, as far as
        # she knows: pits, one-way drops; where she stands may itself be a spot she can't get
        # back to, so it's the share of the rest that counts)
        doors = view.local_doors(keys)
        back: dict[int, bool] = {}

        def returnable(o: Objective) -> bool:
            if o.kind in ("exit", "teleporter", "key"):
                return True  # (worth the risk: where it leads is the unknown, keys open the way)
            p = nav.mesh.poly_at(*o.goal)
            if p is None:
                return False
            if p.index not in back:
                after = view.reachable(p.index, states_all, doors)
                back[p.index] = here.index in after or len(after & reach) >= KEEP_REACH * len(reach)
            return back[p.index]
        def usable(objectives: list[Objective]) -> list[Objective]:
            if risky:
                # nothing safe left: anything, one-way drops and nukage and all; but not for a
                # sound (MAP09: a noise far off took her down into a pit, with the room
                # down the other one-way drop, the way on, still to look into)
                return [r for o in objectives if o.kind != "noise"
                        and fresh(r := replace(o, label=o.label + " (risky)", risky=True))]
            return [o for o in objectives if fresh(o) and returnable(o)
                    and not (self.dangerous is not None and self.dangerous(obs, o))]
        # with nothing else left, the ways between rooms never walked over (walk-over lines
        # are thresholds: E4M7's was between two rooms she'd come into other ways)
        # (and last of all, those onto floors that hurt: E4M2's door opener is on lava)
        return (usable(out) or usable(self._thresholds(reach)) or usable(self._thresholds(reach, hurting=True))
                or usable(again)
                or (usable(self.last_resort(obs, reach, pos)) if self.last_resort is not None else []))

    def _frontier(self, reach: set[int], pos) -> list[Objective]:
        """Known polys she can get to next to polys she hasn't seen into: per sector those are
        in, at the reachable poly nearest her (done once she's seen into any of them)."""
        mesh, k = self.nav.mesh, self.knowledge
        known = k.known_polys()
        best: dict[int, tuple[float, tuple]] = {}
        unseen: dict[int, set[int]] = {}
        for pi in reach:
            poly = mesh.polys[pi]
            for portal in poly.portals:
                q = mesh.portals[portal]
                other = mesh.polys[q.other(pi)]
                if other.index in known:
                    continue
                unseen.setdefault(other.sector, set()).add(other.index)
                # look from the way in, just this side of it (from the poly's middle it can be
                # round a corner: E2M4)
                mid = ((q.p1[0] + q.p2[0]) / 2, (q.p1[1] + q.p2[1]) / 2)
                back = math.dist(mid, poly.center)
                k = min(1.0, FRONTIER_BACK / back) if back > 0 else 0.0
                spot = (mid[0] + (poly.center[0] - mid[0]) * k, mid[1] + (poly.center[1] - mid[1]) * k)
                if mesh.poly_at(*spot) is None or mesh.poly_at(*spot).index != pi:
                    spot = poly.center
                d = math.dist(spot, pos)
                if other.sector not in best or d < best[other.sector][0]:
                    best[other.sector] = (d, spot)
        return [Objective("frontier", self._stand_spot(c, reach), f"look into sector {s}", reveals=frozenset(unseen[s]))
                for s, (_, c) in best.items()]

    def _thresholds(self, reach: set[int], hurting: bool = False) -> list[Objective]:
        """Reachable polys she hasn't stood in across a line from ones she has: just over the
        line (all a walk-over line needs, and a strip's middle may have no room for the box:
        E4M7's, against a platform)."""
        k, mesh = self.knowledge, self.nav.mesh
        out = []
        for pi in sorted(reach - k.visited):
            poly = mesh.polys[pi]
            if k.damaging(poly.sector) != hurting:
                continue
            for q in poly.portals:
                portal = mesh.portals[q]
                if portal.linedef is None or portal.other(pi) not in k.visited:
                    continue
                mid = ((portal.p1[0] + portal.p2[0]) / 2, (portal.p1[1] + portal.p2[1]) / 2)
                d = math.dist(mid, poly.center)
                if d < 1e-6:
                    continue
                spot = (mid[0] + (poly.center[0] - mid[0]) / d * THRESHOLD_IN,
                        mid[1] + (poly.center[1] - mid[1]) / d * THRESHOLD_IN)
                at = mesh.poly_at(*spot)
                if at is None or at.index != pi or not self._standable(spot):
                    continue  # (a sliver by a wall or shut door the box can't stand in)
                out.append(Objective("visit", spot, f"step over line {portal.linedef}",
                                     reveals=frozenset({pi})))
                break
        return out

    def _standable(self, q) -> bool:
        """Can the box stand at q, come to from the floor there: clear of walls and things,
        room overhead, and not hanging over a floor more than a step higher (MAP16's goal had
        the box's edge up on a ledge 96 high)?"""
        clear, floor, ceiling = self.nav.jump_lab().fits_at(*q)
        here = self.nav.mesh.poly_at(*q)
        centre = self.nav.movers.initial[here.sector][0] if here is not None else floor
        return clear and ceiling - floor >= PLAYER_HEIGHT and floor <= centre + MAX_STEP

    def touch_spot(self, item, reach: set[int]):
        """Where to stand to pick up a thing at `item`: there if the box can stand there, else
        the nearest spot it can stand at, in a poly she can get to, close enough for the boxes
        to touch (a pickup's box and hers overlap: 36 either way), on a floor near the item's;
        None if there isn't one (MAP10's soulsphere in an alcove: the middle of the room next
        to it was 100 off, and stood at, picked up nothing)."""
        mesh = self.nav.mesh
        at = mesh.poly_at(*item)
        floor = self.nav.movers.initial[at.sector][0] if at is not None else None
        for r in (0.0, 8.0, 16.0, 24.0, 32.0):
            for k in range(1 if r == 0 else 16):
                q = (item[0] + r * math.cos(k * math.pi / 8), item[1] + r * math.sin(k * math.pi / 8))
                if max(abs(q[0] - item[0]), abs(q[1] - item[1])) >= TOUCH:
                    continue
                poly = mesh.poly_at(*q)
                if poly is None or poly.index not in reach or not self._standable(q):
                    continue
                if floor is not None and abs(self.nav.movers.initial[poly.sector][0] - floor) > MAX_STEP:
                    continue
                return q
        return None

    def _stand_spot(self, spot, reach: set[int]):
        """`spot`, if the box can stand there; else the middle of its poly, or of one next to
        it she can get to, where it can (E3M4: a look-out in a thin strip by a shut door,
        tried for ever)."""
        mesh = self.nav.mesh
        standable = self._standable
        if standable(spot):
            return spot
        poly = mesh.poly_at(*spot)
        if poly is None:
            return spot
        around = [poly.index] + [mesh.portals[q].other(poly.index) for q in poly.portals]
        for pi in around:
            if pi in reach and standable(mesh.polys[pi].center):
                return mesh.polys[pi].center
        return spot

    def _teleporters(self, reach: set[int], pos) -> list[Objective]:
        """Teleporters she has seen and not been through: walk over one (from the side that
        sends you) to find out where it goes. One objective per pad (the sides of one go to
        the same place), over its side nearest her."""
        from evildoom.nav.reach import Teleport
        from evildoom.wad.specials import classify_line
        k, nav = self.knowledge, self.nav
        tried = set(k.teleports)
        best: dict[frozenset, Objective] = {}
        for li in k.teleporters():
            if li in tried:
                continue
            src = {p for p, tps in nav.tele_from.items() for tp in tps
                   if isinstance(tp, Teleport) and tp.line == li} & reach
            if not src:
                continue
            line = nav.map.linedefs[li]
            front, back = nav.map.front_sector(line), nav.map.back_sector(line)
            now = self.states()
            if front is not None and back is not None and not any(
                    can_cross(a, b) for a in now[front] for b in now[back]):
                continue  # (the pad isn't down yet, as far as she's seen)
            a, b = nav.map.vertexes[line.v1], nav.map.vertexes[line.v2]
            length = max(math.dist((a.x, a.y), (b.x, b.y)), 1.0)
            mid = ((a.x + b.x) / 2, (a.y + b.y) / 2)
            # the front side (the one that teleports), a little back from the line
            spot = (mid[0] + (b.y - a.y) / length * 24, mid[1] - (b.x - a.x) / length * 24)
            if nav.mesh.poly_at(*spot) is None or nav.mesh.poly_at(*spot).index not in reach:
                spot = min((nav.mesh.polys[p].center for p in src), key=lambda c: math.dist(c, mid))
            t = TriggerLine(li, classify_line(line.special), [], {}, src)
            pad = k.teleport_pad(li)
            o = Objective("teleporter", spot, f"try teleporter (line {li})", trigger=t, pad=pad)
            if pad not in best or math.dist(spot, pos) < math.dist(best[pad].goal, pos):
                best[pad] = o
        return list(best.values())

    def _noises(self, reach: set[int], pos, tic: int) -> list[Objective]:
        """Sounds she heard but couldn't see the source of, lately: go to the reachable spot
        nearest where it seemed to come from (if that's nearer than here) and look."""
        mesh, k = self.nav.mesh, self.knowledge
        out = []
        for h in k.heard:
            if tic - h.tic > NOISE_TICS or not self._worth_a_look(h) or self._noise_seen(h):
                continue
            urgent = self._urgent(h, tic)
            best = min(reach, key=lambda p: math.dist(mesh.polys[p].center, h.origin), default=None)
            if best is None:
                continue
            spot = mesh.polys[best].center
            if math.dist(spot, h.origin) > math.dist(pos, h.origin) - HEAR_NEARER:
                continue
            out.append(Objective("noise", spot, f"check a noise {_compass(h.bearing)} (~{h.distance:.0f})"
                                 + (" right away" if urgent else ""),
                                 reveals=frozenset({h.tic}), heard=h, urgent=urgent))
        return out

    def _urgent(self, h, tic: int) -> bool:
        """A sound heard just after she set off a line she doesn't know the effect of: go and
        look now, while it's still moving (MAP19's line 443 opens a timed door out of sight,
        the way on; looked at later, it was shut again and nothing was learned)."""
        k = self.knowledge
        if tic - h.tic >= URGENT_TICS or h.seen:
            return False
        lines = {f.line for f in k.fired[-8:] if 0 <= h.tic - f.tic <= CAUSE_TICS}
        return any(not k.learned.get(i) for i, t in enumerate(self.nav.triggers) if t.line in lines)

    def _noise_seen(self, h) -> bool:
        """Has where a sound seemed to come from been in view since?"""
        poly = self.nav.mesh.poly_at(*h.origin)
        return poly is not None and self.knowledge.seen_tic.get(poly.sector, -1) > h.tic

    def _unfinished(self, reach: set[int], pos) -> list[Objective]:
        """Rooms not closed on the map yet: for each known sector with walls not seen, a
        reachable spot it can be seen from (checked by looking from there, at the heights as
        last seen), nearest her first."""
        mesh, m, k = self.nav.mesh, self.nav.map, self.knowledge
        heights = [tuple(k.seen_heights.get(i, (sec.floor, sec.ceiling)))
                   for i, sec in enumerate(m.sectors)]
        # (only spots the box can stand at: E4M7's was in a shut door)
        centres = [(mesh.polys[p].center, p) for p in reach if self._standable(mesh.polys[p].center)]
        out = []
        for s, lines in k.unseen_walls().items():
            best = None
            for li in lines:
                line = m.linedefs[li]
                a, b = m.vertexes[line.v1], m.vertexes[line.v2]
                mid = ((a.x + b.x) / 2, (a.y + b.y) / 2)
                near = sorted((c for c in centres if math.dist(c[0], mid) <= ROOM_LOOK_RANGE),
                              key=lambda c: math.dist(c[0], mid))[:ROOM_SPOTS]
                for c, _ in near:
                    angle = math.degrees(math.atan2(mid[1] - c[1], mid[0] - c[0]))
                    drawn, _, _ = k._sight.drawn(c[0], c[1], angle, k.width, heights)
                    if li in drawn:
                        key = math.dist(c, pos) + math.dist(c, mid)
                        if best is None or key < best[0]:
                            best = (key, li, c, mid)
                        break
            if best is not None:
                _, li, c, mid = best
                out.append(Objective("room", c, f"look at a wall of sector {s} not seen yet",
                                     reveals=frozenset({li}), look_at=mid))
        return out

    def _unvisited(self, reach: set[int], pos) -> list[Objective]:
        """Parts of known rooms she can get to but hasn't walked (walking round finds the
        walk-over lines nobody can see, often right across a room): per sector, the reachable
        poly furthest from anywhere she's stood, if that's far enough to be worth it."""
        k, mesh = self.knowledge, self.nav.mesh
        stood = [mesh.polys[p].center for p in k.visited]
        entered = {mesh.polys[p].sector for p in k.visited}
        best: dict[int, tuple[float, int]] = {}
        nearest: dict[int, tuple[float, int]] = {}  # per sector never stepped into
        for pi in reach:
            poly = mesh.polys[pi]
            if pi in k.visited or k.damaging(poly.sector):
                continue  # (not wading through nukage just to look)
            if not self._standable(poly.center):
                continue  # (the box can't stand there: an 8-wide strip by a shut door, E1M4)
            if poly.sector not in entered:
                d = math.dist(poly.center, pos)
                if poly.sector not in nearest or d < nearest[poly.sector][0]:
                    nearest[poly.sector] = (d, pi)
            gap = min((math.dist(poly.center, c) for c in stood), default=math.inf)
            if gap < COVER_GAP:
                continue
            if poly.sector not in best or gap > best[poly.sector][0]:
                best[poly.sector] = (gap, pi)
        # every room stepped into once, however near she's been: walk-over lines are often
        # across the way in (E1M4's door opener, at the mouth of an alcove she'd stood by)
        for s, (_, pi) in nearest.items():
            best.setdefault(s, (0.0, pi))
        return [Objective("visit", mesh.polys[pi].center, f"walk across sector {s}",
                          reveals=frozenset({pi}))
                for s, (_, pi) in best.items()]

    def _recheck(self, reach: set[int], pos) -> list[Objective]:
        """Since a press whose effect wasn't seen: the known sectors round the reachable area
        not seen since (a door that opened out of sight, a floor that came down), one per
        sector, at the reachable poly next to it nearest her."""
        k, mesh = self.knowledge, self.nav.mesh
        since = k.unexplained_since()
        if since is None:
            return []
        reach_sectors = {mesh.polys[p].sector for p in reach}
        best: dict[int, tuple[float, tuple]] = {}
        for pi in reach:
            poly = mesh.polys[pi]
            for portal in poly.portals:
                s = mesh.polys[mesh.portals[portal].other(pi)].sector
                if s in reach_sectors or s not in k.sectors or k.seen_tic.get(s, -1) > since:
                    continue
                d = math.dist(poly.center, pos)
                if s not in best or d < best[s][0]:
                    best[s] = (d, poly.center)
        return [Objective("recheck", self._stand_spot(c, reach), f"look at sector {s} again", reveals=frozenset({s}))
                for s, (_, c) in best.items()]

    def ranked(self, obs: Observation, risky: bool = False) -> list[Objective]:
        """The objectives open, the explorer's way round: the exit first if it can get there,
        then the locks her keys open, then by distance as the crow flies (keys and untried
        switches a little ahead of the edge of the map)."""
        options = self.objectives(obs, risky)
        if not self._exits_open:
            # (nor anywhere she'd slide over one getting there: MAP24's pad, 38 off a goal)
            self._exit_pads()
            options = [o for o in options if all(_segment_distance(a, b, o.goal, o.goal) >= EXIT_MARGIN
                                                  for a, b in self._exit_segs)]
        pos = (obs.x, obs.y)
        options.sort(key=lambda o: (o.priority, math.dist(o.goal, pos) * WEIGHT[o.kind]))
        return options

    def choose(self, obs: Observation, risky: bool = False) -> Objective | None:
        options = self.ranked(obs, risky)
        return options[0] if options else None

    def _wading(self, waypoints) -> float:
        """How far a path goes on floors she takes to hurt: not over them (a hurting pit well
        below a walkway with gaps she runs along, or jumps across: MAP24)."""
        mesh, k = self.nav.mesh, self.knowledge
        heights = self.states()
        out = 0.0
        prev = None
        on = None  # the floor she's on, last time she wasn't over a hurting one
        for p in waypoints:
            if p is None or prev is None:
                prev = p
                continue
            n = max(1, int(math.dist(prev, p) // 16))
            step = math.dist(prev, p) / n
            for i in range(n):
                q = (prev[0] + (p[0] - prev[0]) * (i + 0.5) / n, prev[1] + (p[1] - prev[1]) * (i + 0.5) / n)
                poly = mesh.poly_at(*q)
                if poly is None:
                    continue
                floor = max(h[0] for h in heights[poly.sector])
                if not k.damaging(poly.sector):
                    on = floor
                elif on is None or floor >= on - MAX_STEP:
                    out += step  # (down in it, not passing over it)
            prev = p
        return out

    def route_to(self, obj: Objective, obs: Observation) -> list[Step] | None:
        """Steps to get to an objective and do it, through what's known (using switches
        she has learned on the way if need be)."""
        view = self.view()
        keys = frozenset(obs.keys or ())
        states = self.states()
        k = self.knowledge
        key_items: dict = {}
        for th in k.known_things():
            if th.type in Key._value2member_map_:
                key_items.setdefault(Key(th.type), []).extend(view.pickup_spots(th.x, th.y, states))
        # (the one-shot lines she's used this life are spent: stairs a press raises another
        # step; and a door one opened that something else shut again won't open for it twice:
        # E3M4's door, opened for good by one switch, shut by a press of its own door switch)
        by_line = {t.line: t for t in self.nav.triggers if t.line >= 0}
        fired = frozenset(f.line for f in k.fired if f.life == k.lives
                          if (t := by_line.get(f.line)) is not None and t.once)
        route = view.plan((obs.x, obs.y), [obj.goal], keys, states, key_items,
                          max_expansions=2000, max_seconds=PLAN_SECONDS, fired=fired)
        # no way to the nearest spot to press it from: the next nearest (MAP08's exit door,
        # nearest from a nukage pit through a gap too tight, is pressed from the ledge above)
        goal, tried = obj.goal, set()
        while route is None and obj.trigger is not None and obj.kind in ("switch", "door", "exit")                 and len(tried) < 3 and (at := self.nav.mesh.poly_at(*goal)) is not None:
            tried.add(at.index)
            rest = (obj.trigger.activators & self._reach) - tried
            if not rest:
                break
            goal = self.nav._activation_point(obj.trigger, (obs.x, obs.y), rest)
            route = view.plan((obs.x, obs.y), [goal], keys, states, key_items,
                              max_expansions=2000, max_seconds=PLAN_SECONDS, fired=fired)
        if route is None:
            return None
        if obj.kind in ("frontier", "recheck", "visit", "noise", "room", "item", "hunt"):
            last = Step(None, "goto", label=obj.label)
        elif obj.kind == "key":
            last = Step(None, "pickup", key=obj.key, label=obj.label)
        elif obj.kind == "teleporter":
            last = Step(None, "cross", obj.trigger, label=obj.label)
        else:
            action = _action_for(obj.trigger)
            last = Step(None, "exit" if obj.kind == "exit" and action == "use" else action,
                        obj.trigger, label=obj.label)
        if obj.kind != "exit" and not obj.risky and not self.knowledge.protected(obs.tic):
            # (nukage and the like only for a few steps, and never dropping into it: a pit she
            # believes she can get out of may well be a death trap; but to leave, or when
            # there's nothing else)
            wade = 0.0
            # (dropping in is all right where she's been down before and got out again: MAP24's
            # nukage, left by its teleporters)
            been = {self.nav.mesh.polys[p].sector for p in self.knowledge.visited}
            here = self.nav.mesh.poly_at(obs.x, obs.y)
            escaped = been - ({here.sector} if here is not None else set())
            for leg in route.legs:
                if any("one-way drop" in n and self.knowledge.damaging(s := int(n.rsplit(" ", 1)[1]))
                       and s not in escaped for n in leg.path.steps):
                    return None
                wade += self._wading(leg.path.waypoints)
            if wade > WADE_LIMIT or (wade > 0 and obs.health is not None and obs.health < WADE_HEALTH):
                return None
        if obj.kind != "exit" and not obj.risky and self._shuts_in(route, obj, obs, view):
            return None
        return steps_from_legs(route.legs, last, view, states)

    def _shuts_in(self, route, obj: Objective, obs: Observation, view: Navigator) -> bool:
        """Would the walk-over lines the route goes over cut her off at the end of it, where
        she wouldn't be without them? (MAP08: the line by the red skull drops the floor she'd
        come back over into a pit; a key is worth a one-way drop, not a trap.) The planner
        counts what it sets off on purpose; what a path happens to go over, it doesn't."""
        mesh = self.nav.mesh
        end = mesh.poly_at(*obj.goal)
        here = mesh.poly_at(obs.x, obs.y)
        if end is None or here is None or not route.legs:
            return False
        doors = view.local_doors(set(route.keys))
        # (the planner fires what it means to; walk-over lines the path just goes over fire too)
        k, m = self.knowledge, self.nav.map
        spent = {f.line for f in k.fired if f.life == k.lives}
        legs = [(p, q) for leg in route.legs for p, q in zip(leg.path.waypoints, leg.path.waypoints[1:])
                if p is not None and q is not None]
        final = route.states
        for t in view.triggers:
            if t.line < 0 or not t.effects or t.info.trigger not in (Trigger.W1, Trigger.WR)                     or (t.once and t.line in spent):
                continue
            a, b = _line_ends(m, t.line)
            if any(_segments_cross(p, q, a, b) for p, q in legs):
                final = apply_trigger(final, t)
        if final is route.states:
            return False

        def opened(st: HeightStates) -> HeightStates:
            # (doors gone through on the way: open on the way back too, as far as this goes;
            # MAP08's from the key room's side only opens from the other)
            out = dict(st)
            for d, (open_heights, _) in doors.items():
                out[d] = set(out.get(d, ())) | set(open_heights)
            return out
        # (with what she knows how to work from there: MAP08's switch by the red skull raises
        # the floor the trap dropped, once she's learned it does)
        actions = [t for t in view.triggers if t.effects]
        keys = frozenset(route.keys)
        before, _, _ = view._saturate(end.index, opened(route.states), keys, {}, actions)
        after, _, _ = view._saturate(end.index, opened(final), keys, {}, actions)
        if here.index in after or len(after) >= KEEP_REACH * len(before):
            return False
        # (shut in with something left to try is the way on, maybe: unseen parts, switches not
        # pressed; MAP08's sinking floors to the exit door, the trap room's switch that undoes it)
        known = k.known_polys()
        mesh_portals = mesh.portals
        if any(mesh_portals[q].other(pi) not in known for pi in after for q in mesh.polys[pi].portals):
            return False
        by_line = {t.line: t for t in self.nav.triggers if t.line >= 0}
        if any((t := by_line.get(li)) is not None and t.activators & after for li in k.interactables()):
            return False
        if self.current is None or self.current.ident != obj.ident:
            self.log.append(f"tic {obs.tic}: '{obj.label}': what the way there sets off would "
                            f"shut her in ({len(after)} polys to get to after, {len(before)} before)")
        return True

    # --- playing ------------------------------------------------------------------------

    def restart(self) -> None:
        """Died: the level from the start again, keeping what's known of it (Knowledge.restart).
        Whatever she was doing is dropped; the next pick is the exit if she knows the way."""
        self.knowledge.restart()
        self.log.append(f"--- died: life {self.knowledge.lives}")
        self.current = self.follower = None
        self._dropped.clear()
        self._drop_count.clear()
        self._look_until = self._glance_until = None
        self._heard_n = len(self.knowledge.heard)
        self._glanced = len(self.knowledge.fired)
        self._sector = None
        self._view = None
        self._held_since = None
        self._urgent_n = len(self.knowledge.heard)

    def hold(self, obs: Observation, last: Controls | None = None) -> None:
        """Something else has the controls this tic (a fight): take in what's seen, and
        carry on from here afterwards (the time it took isn't time spent on the objective)."""
        self.knowledge.update(obs, last)
        if self._held_since is None:
            self._held_since = obs.tic

    def act(self, obs: Observation, last: Controls | None = None) -> Controls:
        if self._held_since is not None:
            self._since += obs.tic - self._held_since
            self._held_since = None
            if self.follower is not None:
                self.follower.resume(obs.tic)
        self.knowledge.update(obs, last)
        here = self.nav.mesh.poly_at(obs.x, obs.y)
        if here is not None and here.sector != self._sector:
            if self.nav.damage[here.sector]:
                self.log.append(f"tic {obs.tic}: onto damaging floor (sector {here.sector}) from "
                                f"sector {self._sector}, doing: "
                                f"{self.follower.objective if self.follower else '-'}")
            self._sector = here.sector
        f = self.follower
        # (no looking round in the middle of a jump: MAP08's, glanced away from at the run-up,
        # overshot onto a floor already sinking into the nukage)
        busy = f is not None and (f._sprint is not None or f._run is not None or f._lineup_tic is not None)
        glance = None if busy else self._glance(obs)
        if glance is not None:
            return glance
        if self.current is not None and self.follower is not None and self.follower.done:
            look = self._arrived(obs)
            if look is not None:
                return look
        if self._need_new(obs) and (self.current is not None or self.follower is not None
                                    or obs.tic - self._idle_since >= IDLE_RETRY):
            self._next(obs)
            if self.current is None:
                self._idle_since = obs.tic
        if self.follower is None:
            return Controls(turn=10.0)  # nothing to do: look round
        f = self.follower
        out = f.act(obs)
        # (what the follower had to say: jumps, waits, trouble)
        events = f.log.events
        start = self._follower_seen if self._follower_of is f else 0
        for ev in events[start:]:
            self.log.append(f"    {ev}")
        self._follower_of, self._follower_seen = f, len(events)
        return out

    def _glance(self, obs: Observation) -> Controls | None:
        """Stop and look: towards a sound she couldn't see the source of, or round about when
        something went off underfoot and nothing's been seen or heard to happen (a lift
        that's down for a moment, a door opening behind)."""
        k = self.knowledge
        if self._glance_until is not None:
            if obs.tic < self._glance_until:
                return self._turn_to(obs, self._glance_at)
            self._glance_until = None
            if self.follower is not None:
                self.follower.resume(obs.tic)  # (MAP24: "stuck" after looking round off a teleporter)
        while self._heard_n < len(k.heard):
            h = k.heard[self._heard_n]
            self._heard_n += 1
            if self._worth_a_look(h) and obs.tic - self._last_glance >= GLANCE_GAP:
                return self._start_glance(obs, h.bearing, f"heard something, bearing {h.bearing:.0f}")
        learned = {self.nav.triggers[i].line for i, s in k.learned.items() if s}
        while self._glanced < len(k.fired):
            f = k.fired[self._glanced]
            if f.how == "walk" and f.line not in learned and obs.tic - f.tic < GLANCE_AFTER:
                return None  # (give it a moment to be seen first)
            self._glanced += 1
            if f.how == "walk" and f.line not in learned:
                return self._start_glance(obs, None, f"set off line {f.line} walking")
        return None

    def _worth_a_look(self, h) -> bool:
        """A sound she couldn't see the source of, not one she'd expect, not background."""
        return not h.seen and not self.knowledge.expected(h) and not self._ambient(h)

    def _start_glance(self, obs: Observation, bearing: float | None, why: str) -> Controls:
        self._last_glance = obs.tic
        self.log.append(f"tic {obs.tic}: {why}: looking " + ("round" if bearing is None else "that way"))
        self._glance_until = obs.tic + LOOK_TICS
        self._glance_at = bearing
        return self._turn_to(obs, bearing)

    @staticmethod
    def _turn_to(obs: Observation, bearing: float | None) -> Controls:
        """Turn towards `bearing` (or all the way round, for None)."""
        if bearing is None:
            return Controls(turn=360.0 / LOOK_TICS)
        diff = (bearing - obs.angle + 180) % 360 - 180  # (positive: anticlockwise, to the left)
        return Controls(turn=max(-TURN_RATE, min(TURN_RATE, -diff)))

    def _ambient(self, h) -> bool:
        """A sound heard again and again from the same place (a lift going up and down for
        ever): tuned out."""
        same = [o for o in self.knowledge.heard
                if math.dist(o.origin, h.origin) < AMBIENT_RADIUS and o.tic <= h.tic]
        return len(same) > AMBIENT_REPEATS

    def _seen_to(self, obj: Objective) -> bool:
        """Has what the objective was to look at been seen?"""
        k = self.knowledge
        if obj.kind == "frontier":
            return bool(obj.reveals & k.known_polys())
        if obj.kind == "recheck":
            return all(k.seen_tic.get(s, -1) >= self._since for s in obj.reveals)
        if obj.kind == "noise":
            return self._noise_seen(obj.heard)
        if obj.kind == "room":
            return obj.reveals <= k.mapped
        if obj.kind == "visit":
            return obj.reveals <= k.visited
        return False

    def _arrived(self, obs: Observation) -> Controls | None:
        """The route to the objective is done: check a press went off, and look round where
        there's something to look for (what can be seen from there, what a switch did).
        Controls while still looking, else None (and a new objective's wanted)."""
        obj = self.current
        k = self.knowledge
        if obj.kind in ("switch", "door") and not any(
                k.mechanism(f.line) == k.mechanism(obj.trigger.line) for f in k.fired):
            self._drop(obj, obs, "pressed, but nothing went off")
        elif obj.kind in ("frontier", "switch", "recheck", "noise", "room") and not self._seen_to(obj):
            if self._look_until is None:
                self._look_until = obs.tic + LOOK_TICS
            if obs.tic < self._look_until:
                at = obj.heard.origin if obj.kind == "noise" else obj.look_at
                if at is not None:
                    return self._turn_to(obs, math.degrees(math.atan2(at[1] - obs.y, at[0] - obs.x)))
                return Controls(turn=360.0 / LOOK_TICS)
            if obj.kind != "switch" and not self._seen_to(obj):
                self._drop(obj, obs, "nothing to see from there")
        elif obj.kind == "visit" and not self._seen_to(obj):
            self._drop(obj, obs, "got there but not in it")
        elif obj.kind == "teleporter" and obj.trigger.line not in self.knowledge.teleports:
            self._drop(obj, obs, "didn't take me anywhere")
        elif obj.kind in ("item", "hunt") and not (self.gone is not None and self.gone(obs, obj)):
            if obj.kind == "item":
                # Doom picks things up only moving onto them: come down on one from a ledge (in
                # the air, it's out of reach) and stand still, and it stays (MAP10's bonuses in
                # the nukage pits). A shuffle over it first.
                if self._look_until is None:
                    self._look_until = obs.tic + SHUFFLE_TICS
                if obs.tic < self._look_until:
                    a = math.atan2(obj.goal[1] - obs.y, obj.goal[0] - obs.x) - math.radians(obs.angle)
                    push = 20.0 if (obs.tic // 4) % 2 == 0 else -20.0
                    return Controls(forward=push * math.cos(a), strafe=-push * math.sin(a))
            self._drop(obj, obs, "not there" if obj.kind == "hunt" else "couldn't pick it up")
        self._look_until = None
        self.follower = None
        return None

    def _note_misses(self, obs: Observation) -> bool:
        """Run-jumps the follower missed: counted (for the level, across lives), and left out
        of the navigator from now on (E4M2: a jump onto a ledge a few units wide, made from a
        run-up against a wall, came down in the nukage each time). True if one was."""
        f = self.follower
        if f is None or not f.missed:
            return False
        for j in f.missed:
            n = self.knowledge.missed_jumps[j.key] = self.knowledge.missed_jumps.get(j.key, 0) + 1
            self.log.append(f"tic {obs.tic}: missed the jump off ({j.takeoff[0]:.0f}, {j.takeoff[1]:.0f}) "
                            + ("for good: not trying it again" if n >= JUMP_MISSES else "once: another way first"))
        f.missed.clear()
        self._set_jumps(difficult=False)
        return True

    def _set_jumps(self, difficult: bool) -> None:
        """The navigator's run-jumps: all but those missed too often (and, unless `difficult`,
        those missed at all)."""
        if self._all_jumps is None:
            self._all_jumps = list(self.nav.jumps)
        missed = self.knowledge.missed_jumps
        limit = JUMP_MISSES if difficult else 1
        self.nav.set_jumps([j for j in self._all_jumps if missed.get(j.key, 0) < limit])
        self._difficult = difficult
        self._view = None  # (it holds the old edges)

    def _need_new(self, obs: Observation) -> bool:
        if self._note_misses(obs):
            return True  # (the way it was going may take that jump again: plan anew)
        f, obj = self.follower, self.current
        if f is None or obj is None or f.done or f.failed:
            if f is not None and f.failed and obj is not None:
                why = next((str(ev) for ev in reversed(f.log.events) if "FAILED" in str(ev)), "")
                self._drop(obj, obs, "failed: " + why.split("] ", 1)[-1])
            return True
        if obj.kind == "frontier" and obj.reveals & self.knowledge.known_polys():
            return True  # seen it on the way
        if obj.kind == "teleporter" and obj.trigger.line in self.knowledge.teleports:
            return True  # been through
        if obj.kind != "exit" and not self.exit_last and len(self.knowledge.exits()) > self._exits_seen:
            return True  # an exit came into view
        if self.gone is not None and self.gone(obs, obj):
            return True  # (picked up, killed)
        heard = self.knowledge.heard
        fresh, self._urgent_n = heard[self._urgent_n:], len(heard)
        # (something just went off out of sight: go and look first; not half way through a
        # run of jumps, though: MAP08's, broken off on a landing, walked off it into a pit)
        jumping = any("run-jump" in n for st in f.steps[f.step_i:] if st.path for n in st.path.steps)
        if not (obj.kind == "noise" and obj.urgent) and not jumping and any(
                self._urgent(h, obs.tic) and self._worth_a_look(h) for h in fresh):
            return True
        if not jumping and self.interrupt is not None and self.interrupt(obs, obj):
            return True
        if obs.tic - self._since > self._allowed:
            self._drop(obj, obs, "took too long")
            return True
        return False

    def _drop(self, obj: Objective, obs: Observation, why: str) -> None:
        self._dropped[obj.ident] = obs.tic
        self._drop_count[obj.ident] = self._drop_count.get(obj.ident, 0) + 1
        self.log.append(f"tic {obs.tic}: dropped '{obj.label}' ({why})")

    def _exit_pads(self) -> set[int]:
        """The polys beyond walk-over exit lines: of the two sides of each, the sector more of
        whose edge is exit lines (MAP24's pad, ringed by five: walked onto looking round, the
        level was over with 0 of 49 items)."""
        if self._pads is None:
            m, mesh = self.nav.map, self.nav.mesh
            exit_lines = [li for li, line in enumerate(m.linedefs)
                          if (info := classify_line(line.special)) is not None
                          and info.category in (Category.EXIT, Category.SECRET_EXIT)
                          and info.trigger in (Trigger.W1, Trigger.WR)]
            def share(sec: int) -> float:
                edge = [li for li, line in enumerate(m.linedefs) if sec in (m.front_sector(line), m.back_sector(line))]
                length = lambda li: math.dist(*((m.vertexes[m.linedefs[li].v1].x, m.vertexes[m.linedefs[li].v1].y),
                                                (m.vertexes[m.linedefs[li].v2].x, m.vertexes[m.linedefs[li].v2].y)))
                total = sum(length(li) for li in edge) or 1.0
                return sum(length(li) for li in edge if li in exit_lines) / total
            pads = set()
            for li in exit_lines:
                sides = [s for s in (m.front_sector(m.linedefs[li]), m.back_sector(m.linedefs[li])) if s is not None]
                if sides:
                    pads.add(max(sides, key=share))
            self._pads = {p.index for p in mesh.polys if p is not None and p.sector in pads}
            self._exit_segs = [((m.vertexes[m.linedefs[li].v1].x, m.vertexes[m.linedefs[li].v1].y),
                                (m.vertexes[m.linedefs[li].v2].x, m.vertexes[m.linedefs[li].v2].y))
                               for li in exit_lines]
        return self._pads

    def _next(self, obs: Observation) -> None:
        self.follower = None
        if self._difficult:
            self._set_jumps(difficult=False)  # (the jumps missed once: out again, other ways first)
        self._exits_open = not self.exit_last
        found = self._try_objectives(obs, risky=False) or self._try_objectives(obs, risky=True)
        if not found and self.exit_last and self.before_exit is not None and self.before_exit(obs):
            self.log.append(f"tic {obs.tic}: nothing else to do here: the level again, for what's left")
            self.restart_wanted = True
            self.current = None
            return
        if not found and self.exit_last:
            self.log.append(f"tic {obs.tic}: nothing else to do: for the exit")
            self._exits_open = True
            found = self._try_objectives(obs, risky=False) or self._try_objectives(obs, risky=True)
        if not found and any(n < JUMP_MISSES for n in self.knowledge.missed_jumps.values()):
            self.log.append(f"tic {obs.tic}: nothing else to do: the jumps missed once get another go")
            self._set_jumps(difficult=True)
            found = self._try_objectives(obs, risky=False) or self._try_objectives(obs, risky=True)
        if not found:
            if self.current is not None:
                self.log.append(f"tic {obs.tic}: nothing left to do")
            self.current = None

    def _try_objectives(self, obs: Observation, risky: bool) -> bool:
        """Take up the best objective there's a route to: of the nearest few as the crow flies,
        the one nearest by the way there (teleporters and lifts make those very different:
        MAP24); False if none has a route."""
        for _ in range(3):
            options = self.ranked(obs, risky)[:COMPARED]
            if not options:
                return False
            routed = []
            for obj in options:
                steps = self.route_to(obj, obs)
                if steps:
                    length = self._route_length(steps)
                    if obj.worth is not None and length > obj.worth:
                        self.log.append(f"tic {obs.tic}: '{obj.label}' not worth {length:.0f} "
                                        f"to get to (at most {obj.worth:.0f}): in its turn")
                        obj = replace(obj, urgent=False, worth=None, label=obj.label.split(" (")[0])
                    routed.append((length * WEIGHT[obj.kind], obj, steps))
                    if obj.priority < 2:
                        break  # (nothing further down the list comes first anyway)
                else:
                    self._drop(obj, obs, "no route")
            if routed:
                routed.sort(key=lambda r: (r[1].priority, r[0]))
                _, obj, steps = routed[0]
                self.current, self._since = obj, obs.tic
                # (time to do it: walking the route, and some)
                self._allowed = max(GIVE_UP_TICS, int(self._route_length(steps) / ROUTE_SPEED) + ROUTE_SLACK)
                self._exits_seen = len(self.knowledge.exits())
                self.follower = RouteFollower(self.view(), steps, keys=set(obs.keys or ()),
                                              states=self.states(), replan_level=False,
                                              tic=obs.tic)
                self.log.append(f"tic {obs.tic}: {obj.label} ({len(steps)} steps) | "
                                f"{self.knowledge.summary()}")
                return True
        return False

    @staticmethod
    def _route_length(steps: list[Step]) -> float:
        return sum(math.dist(a, b) for st in steps if st.path
                   for a, b in zip(st.path.waypoints, st.path.waypoints[1:]) if a and b)


def _line_ends(m, li: int):
    line = m.linedefs[li]
    a, b = m.vertexes[line.v1], m.vertexes[line.v2]
    return (a.x, a.y), (b.x, b.y)
