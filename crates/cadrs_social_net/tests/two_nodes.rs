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
            } else if post.bytes == b"rant" {
                // Megabytes of "reason".
                post.refuse("N".repeat(5 << 20));
            } else {
                post.refuse("no");
            }
        }
    });
    assert_eq!(phone.send(shop.addr(), b"hello").await, Ok(b"hi".to_vec()));
    // Reached, and refused: the error says so before the reason, unlike one for a device that
    // couldn't be reached.
    let refused = phone.send(shop.addr(), b"junk").await;
    assert_eq!(refused, Err(format!("{}no", cadrs_social_net::REFUSED)));
    // The reason is the other device's to give, and so is its length: a sentence of it is
    // read, whatever was sent.
    let ranted = phone.send(shop.addr(), b"rant").await.unwrap_err();
    assert_eq!(
        ranted.chars().count(),
        cadrs_social_net::REFUSED.len() + cadrs_social_net::MAX_REASON
    );
    // And the next post goes as before.
    assert_eq!(phone.send(shop.addr(), b"hello").await, Ok(b"hi".to_vec()));
}
