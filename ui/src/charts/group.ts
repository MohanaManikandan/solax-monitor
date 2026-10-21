/** A chart that shares its time window with others. */
export interface GroupMember {
  /** Shows the window another chart moved to. */
  followWindow(min: number, max: number): void;
  /** Back to the window it opens on. */
  resetWindow(): void;
}

/**
 * Charts read against each other over the same day. Zooming one zooms them
 * all, a double click resets them all, and a zoom let go at the live edge
 * keeps every one of them following new readings.
 */
export class ChartGroup {
  zoomed = false;
  followsLive = false;
  /** Where today's projection ends while it is shown, which every chart in
   * the group runs to; none while it is hidden or there is none. */
  projectionEnd: number | null = null;
  private readonly members = new Set<GroupMember>();

  join(member: GroupMember): void {
    this.members.add(member);
  }

  leave(member: GroupMember): void {
    this.members.delete(member);
  }

  /** One chart's window moved: the others move with it. */
  moved(source: GroupMember, min: number, max: number): void {
    for (const member of this.members) {
      if (member !== source) member.followWindow(min, max);
    }
  }

  /** Every chart back on its opening window, dropping any zoom. */
  reset(): void {
    this.zoomed = false;
    this.followsLive = false;
    for (const member of this.members) member.resetWindow();
  }
}
