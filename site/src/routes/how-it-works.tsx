import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/how-it-works')({
  head: () => ({ meta: [{ title: 'How It Works | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/how-it-works' }] }),
  component: () => <App path='/how-it-works' />,
})
