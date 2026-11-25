# Public documents

*Adapted from Gianluca Corsini's guide to cadrs
([#273](https://github.com/rvdende/cadrs/issues/273)).*

An owner can publish a document with **Share › Public**. Everyone using cadrs can then find it on
the documents page, view it, and fork a copy of their own. Nobody else can edit it, and the
owner's later changes stay private until they publish again. This is separate from
[editing together](editing-together.md), which still goes through the owner.

Three things make it work:

| What | What it is | Where it runs |
| --- | --- | --- |
| The public room | a channel shared by every cadrs that is online; each passes messages on to the others, so an announcement spreads from peer to peer | nowhere in particular: it is the connections between the peers |
| A seed peer (`cadrs --public-seed`) | cadrs without a window: a member of the public room that never leaves, and keeps every listing, star and published revision it hears of | a computer that stays online; the project runs one, which every cadrs knows as its default seed |
| `peers.cadrs.net` | a list of who is online, so a cadrs that just connected can find the room; it holds no documents | the project's server |

When someone publishes, their cadrs announces the document in the public room. The seed hears
it, fetches the published revision from the publisher's computer and keeps a copy. Anyone who
goes online later receives the list of public documents, and can open one even while its owner
is offline: the copy comes from the seed, or from another peer that already has it.

- **What a seed keeps:** only what was published: the listings, the stars and the published
  revisions. Private documents, and documents shared with particular people, never go there.
- **Trust comes from signatures.** Every listing and star is signed, and every revision is
  checked against its signed hash.
- **Staying out.** Every online cadrs takes part in the public room unless public documents are
  turned off in **Preferences › Collaboration**.
- **A seed of your own.** Anyone can run `cadrs --public-seed` on their own computer and add its
  cadrs ID as a seed in **Preferences › Collaboration**. It then keeps a copy of every public
  document it hears of, and stays reachable when the project's server isn't.
