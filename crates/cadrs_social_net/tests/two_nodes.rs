//! Two nodes on this machine, no relay: a post is taken with an answer, another refused.

use cadrs_social_net::Node;

#[tokio::test(flavor = "multi_thread")]
async fn a_post_goes_and_its_answer_comes_back() {
    let (shop, mut incoming) = Node::spawn([1; 32], false).await.unwrap();
    let (phone, _) = Node::spawn([2; 32], false).await.unwrap();
    phone.remember(shop.addr());
    tokio::spawn(async move {
        while let Some(post) = incoming.recv().await {
            if post.bytes == b"hello" {
                post.take(b"hi".to_vec());
            } else {
                post.refuse("no");
            }
        }
    });
    assert_eq!(phone.send(shop.addr(), b"hello").await, Ok(b"hi".to_vec()));
    assert_eq!(phone.send(shop.addr(), b"junk").await, Err("no".into()));
}
