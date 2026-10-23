import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/privacy')({
  head: () => ({ meta: [{ title: 'Privacy | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/privacy' }] }),
  component: () => <App path='/privacy' />,
})
