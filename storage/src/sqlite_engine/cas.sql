UPDATE storage SET value = :new_value WHERE tree = :tree AND key = :key AND value IS :old_value
RETURNING id;
