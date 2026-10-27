-- Thread search over the learner's own chats. Local only, inside the
-- encrypted profile database; nothing here is synced or shared.
--
-- The index holds complete messages only, so a streaming answer is indexed
-- once when it finishes and a failed or cancelled one never is. Rows carry
-- the message id rather than relying on chat_messages' implicit rowid, which
-- a VACUUM may renumber. Triggers keep the index in step with chat_messages,
-- including the cascade when a thread is deleted; they fire only when a
-- complete message is involved, so streaming checkpoints never touch it.
CREATE VIRTUAL TABLE chat_message_search USING fts5(
    message_id UNINDEXED,
    content,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER chat_message_search_insert AFTER INSERT ON chat_messages
WHEN new.status = 'complete'
BEGIN
    INSERT INTO chat_message_search(message_id, content) VALUES (new.id, new.content);
END;

CREATE TRIGGER chat_message_search_update AFTER UPDATE OF content, status ON chat_messages
WHEN old.status = 'complete' OR new.status = 'complete'
BEGIN
    DELETE FROM chat_message_search WHERE message_id = old.id;
    INSERT INTO chat_message_search(message_id, content)
    SELECT new.id, new.content WHERE new.status = 'complete';
END;

CREATE TRIGGER chat_message_search_delete AFTER DELETE ON chat_messages
WHEN old.status = 'complete'
BEGIN
    DELETE FROM chat_message_search WHERE message_id = old.id;
END;
