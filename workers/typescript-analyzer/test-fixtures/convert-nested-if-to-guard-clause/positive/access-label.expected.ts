export function accessLabel(user: { active: boolean; admin: boolean } | null) {
  if (!user) return "guest";
  if (!user.active) return "disabled";
  if (user.admin) return "admin";
  return "member";
}
