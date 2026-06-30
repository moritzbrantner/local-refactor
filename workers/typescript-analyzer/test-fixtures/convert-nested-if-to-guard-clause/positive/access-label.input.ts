export function accessLabel(user: { active: boolean; admin: boolean } | null) {
  if (user) {
    if (user.active) {
      if (user.admin) {
        return "admin";
      }
      return "member";
    }
    return "disabled";
  }
  return "guest";
}
