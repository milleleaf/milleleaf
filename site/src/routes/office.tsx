import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/office')({
  head: () => ({ meta: [{ title: 'Office desk: four editors, one desk | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/office' }] }),
  component: () => <App path='/office' />,
})
