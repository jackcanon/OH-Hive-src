-- Host-local curation overlay: never moves or deletes source files.
CREATE TABLE vault_observations(
 document_id TEXT PRIMARY KEY,
 vault_id TEXT NOT NULL,
 revision TEXT NOT NULL,
 path TEXT NOT NULL,
 first_observed_ms INTEGER NOT NULL,
 changed_ms INTEGER NOT NULL,
 present INTEGER NOT NULL DEFAULT 1
);
INSERT INTO vault_observations
 SELECT id,vault_id,revision,path,CAST(strftime('%s','now') AS INTEGER)*1000,
 CAST(strftime('%s','now') AS INTEGER)*1000,1 FROM vault_documents;
CREATE TABLE vault_provenance(
 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
 vault_id TEXT NOT NULL, document_id TEXT NOT NULL,
 at_ms INTEGER NOT NULL, kind TEXT NOT NULL,
 revision TEXT NOT NULL, path TEXT NOT NULL
);
INSERT INTO vault_provenance(vault_id,document_id,at_ms,kind,revision,path)
 SELECT vault_id,document_id,changed_ms,'baseline',revision,path FROM vault_observations;
CREATE TRIGGER vault_observe_insert AFTER INSERT ON vault_documents BEGIN
 INSERT INTO vault_observations VALUES(new.id,new.vault_id,new.revision,new.path,
 CAST(strftime('%s','now') AS INTEGER)*1000,CAST(strftime('%s','now') AS INTEGER)*1000,1)
 ON CONFLICT(document_id) DO UPDATE SET vault_id=new.vault_id,revision=new.revision,path=new.path,
 changed_ms=CAST(strftime('%s','now') AS INTEGER)*1000,present=1;
 INSERT INTO vault_provenance(vault_id,document_id,at_ms,kind,revision,path)
 VALUES(new.vault_id,new.id,CAST(strftime('%s','now') AS INTEGER)*1000,'indexed',new.revision,new.path);
END;
CREATE TRIGGER vault_observe_update AFTER UPDATE ON vault_documents WHEN old.revision != new.revision BEGIN
 UPDATE vault_observations SET revision=new.revision,path=new.path,
 changed_ms=CAST(strftime('%s','now') AS INTEGER)*1000,present=1 WHERE document_id=new.id;
 INSERT INTO vault_provenance(vault_id,document_id,at_ms,kind,revision,path)
 VALUES(new.vault_id,new.id,CAST(strftime('%s','now') AS INTEGER)*1000,'revised',new.revision,new.path);
END;
CREATE TRIGGER vault_observe_delete AFTER DELETE ON vault_documents BEGIN
 UPDATE vault_observations SET present=0 WHERE document_id=old.id;
 INSERT INTO vault_provenance(vault_id,document_id,at_ms,kind,revision,path)
 VALUES(old.vault_id,old.id,CAST(strftime('%s','now') AS INTEGER)*1000,'source_removed',old.revision,old.path);
END;
CREATE INDEX vault_provenance_doc ON vault_provenance(vault_id,document_id,sequence);
CREATE TABLE vault_archives(
 document_id TEXT PRIMARY KEY, vault_id TEXT NOT NULL,
 revision TEXT NOT NULL, snapshot TEXT NOT NULL, archived_ms INTEGER NOT NULL
);
CREATE TABLE vault_curation_events(
 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
 vault_id TEXT NOT NULL, document_id TEXT NOT NULL,
 at_ms INTEGER NOT NULL, action TEXT NOT NULL,
 revision TEXT NOT NULL, actor TEXT NOT NULL, reason TEXT NOT NULL
);
CREATE INDEX vault_curation_events_vault ON vault_curation_events(vault_id,sequence);
PRAGMA user_version=5;
