import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/data/worldviews')({
  head: () => ({ meta: [{ title: 'Worldviews | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/data/worldviews' }] }),
  component: () => <App path='/data/worldviews' />,
})
