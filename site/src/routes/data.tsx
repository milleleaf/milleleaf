import { createFileRoute, Outlet } from '@tanstack/react-router'
export const Route = createFileRoute('/data')({
  head: () => ({ meta: [{ title: 'Data | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/data' }] }),
  component: Outlet,
})
