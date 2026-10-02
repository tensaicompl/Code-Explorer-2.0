package body Inventory is

   procedure Add (S : in out Store; Sku : String; Count : Quantity) is
      Key : constant Unbounded_String := To_Unbounded_String (Sku);
   begin
      if S.Items.Contains (Key) then
         declare
            Existing : Item := S.Items.Element (Key);
         begin
            Existing.Count := Existing.Count + Count;
            S.Items.Replace (Key, Existing);
         end;
      else
         S.Items.Insert (Key, (Sku => Key, Count => Count, State => Pending));
      end if;
   exception
      when Constraint_Error =>
         raise Program_Error with "quantity overflow for " & Sku;
   end Add;

   function Total (S : Store) return Natural is
      Sum : Natural := 0;
   begin
      for E of S.Items loop
         Sum := Sum + Natural (E.Count);
      end loop;
      return Sum;
   end Total;

   function Max_Of (A, B : Element) return Element is
   begin
      if A < B then
         return B;
      else
         return A;
      end if;
   end Max_Of;

end Inventory;
