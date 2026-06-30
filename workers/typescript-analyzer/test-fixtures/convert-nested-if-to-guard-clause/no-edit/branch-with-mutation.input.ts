export function accessLabel(user: { active: boolean; admin: boolean } | null) {
  if (user) {
    user.active = Boolean(user.active);
    if (user.active) {
      return "member";
    }
  }
  return "guest";
}
