import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/method')({
  head: () => ({ meta: [{ title: 'Method | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/method' }] }),
  component: () => <App path='/method' />,
})
